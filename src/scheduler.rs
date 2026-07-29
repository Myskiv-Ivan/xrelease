//! Concurrency and timing: one supervised polling loop per [`Watch`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use rand::Rng;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::engine::Engine;
use crate::pipeline::{poll_once, Watch};
use crate::store::OUTBOX_MAX_ATTEMPTS;

const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// How long shutdown waits for in-flight polls / flush loop before aborting.
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Final outbox drain after background tasks stop (or after abort on timeout).
const SHUTDOWN_OUTBOX_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Poll interval while waiting for the shutdown flag during sleep/backoff.
const SHUTDOWN_POLL_CHUNK: Duration = Duration::from_millis(250);

/// Replaceable watch-loop supervisor for `xrelease serve` hot-swap.
pub struct WatchSupervisor {
    engine: Arc<Engine>,
    shutdown: Arc<AtomicBool>,
    watchers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl WatchSupervisor {
    /// Create a supervisor bound to `engine` (watchers not started yet).
    #[must_use]
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            shutdown: Arc::new(AtomicBool::new(false)),
            watchers: Mutex::new(Vec::new()),
        }
    }

    /// Start (or replace) polling loops for `watches`, plus the background
    /// advisory sweep over the same set.
    ///
    /// Infallible: stop drains (or aborts after timeout), then spawn replaces
    /// the handle list. Callers that previously treated this as fallible can
    /// drop the `?` — there is no partial failure to compensate.
    ///
    /// The sweep is replaced here rather than started once at boot because it
    /// is bound to the watch list: a config apply that adds, removes, or
    /// re-points a package source must change what gets swept, and it shares
    /// this shutdown flag so `stop_watchers` already drains it.
    pub async fn replace(&self, watches: Vec<Watch>) {
        self.stop_watchers().await;
        self.shutdown.store(false, Ordering::SeqCst);
        let mut handles = spawn_watchers(
            Arc::clone(&self.engine),
            watches.clone(),
            Arc::clone(&self.shutdown),
        );
        handles.extend(spawn_advisory_sweep(
            Arc::clone(&self.engine),
            &watches,
            Arc::clone(&self.shutdown),
        ));
        *self.watchers.lock().await = handles;
    }

    /// Cooperative shutdown of all watch loops.
    pub async fn stop_watchers(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let handles = self.watchers.lock().await.drain(..).collect::<Vec<_>>();
        if handles.is_empty() {
            return;
        }
        let abort_handles: Vec<_> = handles
            .iter()
            .map(tokio::task::JoinHandle::abort_handle)
            .collect();
        match tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, join_all(handles)).await {
            Ok(_) => info!("watch supervisor: loops stopped cleanly"),
            Err(_) => {
                warn!("watch supervisor: timed out; aborting watch loops");
                for handle in abort_handles {
                    handle.abort();
                }
            }
        }
    }
}

/// Long-running background work: poll loops, outbox retry, and DB maintenance.
pub struct BackgroundTasks {
    shutdown: Arc<AtomicBool>,
    maintenance: Option<tokio::task::JoinHandle<()>>,
    outbox: Option<tokio::task::JoinHandle<()>>,
    watchers: Vec<tokio::task::JoinHandle<()>>,
}

impl BackgroundTasks {
    /// Abort every spawned task immediately (tests / emergency stop).
    pub fn abort_all(self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.maintenance {
            handle.abort();
        }
        if let Some(handle) = self.outbox {
            handle.abort();
        }
        for handle in self.watchers {
            handle.abort();
        }
    }

    /// Cooperative shutdown: stop new work, wait for in-flight cycles, drain outbox.
    ///
    /// 1. Flip the shared shutdown flag so watch loops and the flush timer exit
    ///    after their current poll/flush (sleeps are interruptible).
    /// 2. Wait up to [`SHUTDOWN_DRAIN_TIMEOUT`] for those tasks to finish; on
    ///    timeout, abort leftovers via [`tokio::task::AbortHandle`].
    /// 3. Run one final outbox flush so pending rows are not left solely to the
    ///    next process start.
    pub async fn graceful_shutdown(self, engine: &Engine) {
        self.shutdown.store(true, Ordering::SeqCst);
        info!(
            timeout_secs = SHUTDOWN_DRAIN_TIMEOUT.as_secs(),
            "shutdown: waiting for in-flight polls and background loops"
        );

        let mut handles = Vec::with_capacity(
            self.watchers.len()
                + usize::from(self.maintenance.is_some())
                + usize::from(self.outbox.is_some()),
        );
        if let Some(handle) = self.maintenance {
            handles.push(handle);
        }
        if let Some(handle) = self.outbox {
            handles.push(handle);
        }
        handles.extend(self.watchers);

        let abort_handles: Vec<_> = handles
            .iter()
            .map(tokio::task::JoinHandle::abort_handle)
            .collect();
        match tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, join_all(handles)).await {
            Ok(_) => info!("shutdown: background tasks stopped cleanly"),
            Err(_) => {
                warn!("shutdown: timed out waiting for background tasks; aborting leftovers");
                for handle in abort_handles {
                    handle.abort();
                }
            }
        }

        match tokio::time::timeout(SHUTDOWN_OUTBOX_DRAIN_TIMEOUT, engine.flush_outbox(50)).await {
            Ok(Ok(sent)) if sent > 0 => {
                info!(sent, "shutdown: final outbox drain delivered notifications");
            }
            Ok(Ok(_)) => info!("shutdown: final outbox drain complete"),
            Ok(Err(err)) => warn!(error = %err, "shutdown: final outbox drain failed"),
            Err(_) => warn!("shutdown: final outbox drain timed out"),
        }
    }
}

/// Startup recovery plus shared background tasks (outbox + maintenance).
///
/// Production `serve` drives poll loops via [`WatchSupervisor`]; this entry
/// point only starts the process-wide outbox flush and DB maintenance tasks.
pub async fn start_shared_background(engine: Arc<Engine>) -> BackgroundTasks {
    start_background(engine, None).await
}

/// Startup recovery plus optional poller tasks.
///
/// Pass `Some(watches)` to also spawn per-source poll loops (integration tests).
/// Production uses [`start_shared_background`] (`watches = None`) and
/// [`WatchSupervisor`] for hot-swappable loops.
pub async fn start_background(engine: Arc<Engine>, watches: Option<Vec<Watch>>) -> BackgroundTasks {
    startup_outbox_recovery(&engine).await;
    let shutdown = Arc::new(AtomicBool::new(false));
    let watchers = match watches {
        Some(watches) => spawn_watchers(Arc::clone(&engine), watches, Arc::clone(&shutdown)),
        None => Vec::new(),
    };
    BackgroundTasks {
        maintenance: spawn_maintenance(Arc::clone(&engine), Arc::clone(&shutdown)),
        outbox: spawn_outbox_flush(engine, Arc::clone(&shutdown)),
        watchers,
        shutdown,
    }
}

/// Promote rows that used up their retry budget but are still `failed` to
/// `dead`, so they surface in the dead-letter metric / ops alert and become
/// recoverable via `outbox requeue`.
///
/// Must run **periodically**, not only at startup: `claim_outbox_batch` filters
/// on `attempts < OUTBOX_MAX_ATTEMPTS`, so the moment a parent row reaches the
/// cap it stops being claimable. If its status is still `failed` at that point
/// — which happens when the parent's budget runs out before its sink ledger's,
/// e.g. one sink dead-letters for real while another is still pending behind an
/// open circuit breaker — the row is silently invisible: never delivered, never
/// counted as dead-lettered, and skipped by [`Store::requeue_dead_outbox`],
/// which only revives `dead`. Reconciling each flush cycle bounds that window
/// to ~60s instead of "until the next process restart".
pub async fn reconcile_exhausted_outbox(engine: &Engine, reason: &str) {
    match engine.store.finalize_exhausted_outbox() {
        Ok(marked) if marked > 0 => {
            engine.metrics.record_outbox_dead_lettered(marked);
            warn!(
                marked,
                reason,
                max_attempts = OUTBOX_MAX_ATTEMPTS,
                "outbox rows marked dead after exhausting delivery retries"
            );
            engine
                .emit_ops_alert(
                    "Outbox dead-letter sweep",
                    &format!(
                        "{marked} notification outbox row(s) were marked dead after exhausting delivery retries (max_attempts={OUTBOX_MAX_ATTEMPTS})."
                    ),
                )
                .await;
        }
        Err(err) => warn!(error = %err, reason, "outbox dead-letter sweep failed"),
        _ => {}
    }
}

/// Drain retryable outbox rows once at process start (before poll loops).
pub async fn startup_outbox_recovery(engine: &Engine) {
    reconcile_exhausted_outbox(engine, "startup").await;

    match engine.flush_outbox(50).await {
        Ok(sent) if sent > 0 => {
            info!(sent, "startup outbox flush delivered pending notifications");
        }
        Err(err) => warn!(error = %err, "startup outbox flush failed"),
        _ => {}
    }
}

/// Background notification outbox retry loop.
#[must_use]
pub fn spawn_outbox_flush(
    engine: Arc<Engine>,
    shutdown: Arc<AtomicBool>,
) -> Option<tokio::task::JoinHandle<()>> {
    Some(tokio::spawn(async move {
        let period = Duration::from_secs(60);
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            // Isolate each flush in its own task: a *panic* in the delivery
            // path (unlike a returned `Err`) would otherwise terminate this
            // loop for the process's whole life, silently stopping ALL
            // notification retries — polls can't compensate because an already
            // enqueued row is `deliver_now:false`. Catching the join result
            // keeps the loop alive across a panicking flush.
            let flush_engine = Arc::clone(&engine);
            match tokio::spawn(async move { flush_engine.flush_outbox(50).await }).await {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => warn!(error = %err, "notification outbox flush failed"),
                Err(join_err) => {
                    error!(error = %join_err, "notification outbox flush panicked; loop continues")
                }
            }
            // After the flush, not before: a row that just exhausted its budget
            // is reconciled in this same cycle rather than lingering unclaimable
            // and uncounted until the next one.
            reconcile_exhausted_outbox(&engine, "flush cycle").await;
            if !sleep_interruptible(period, &shutdown).await {
                break;
            }
        }
    }))
}

/// Background database retention loop (no-op when disabled).
#[must_use]
pub fn spawn_maintenance(
    engine: Arc<Engine>,
    shutdown: Arc<AtomicBool>,
) -> Option<tokio::task::JoinHandle<()>> {
    let settings = engine.prune_settings().clone();
    if !settings.periodic_enabled() {
        return None;
    }

    let hours = settings.interval_hours;
    info!(interval_hours = hours, "periodic database prune enabled");

    Some(tokio::spawn(async move {
        let period = Duration::from_secs(u64::from(hours) * 3600);
        loop {
            if !sleep_interruptible(period, &shutdown).await {
                break;
            }
            if let Err(err) = engine.run_prune("periodic") {
                warn!(error = %err, "periodic database prune failed");
            }
        }
    }))
}

/// One watched package source resolved to its OSV coordinate.
///
/// Resolved once when the sweep starts rather than per round: the coordinate is
/// a pure function of the watch, and the watch list only changes by way of a
/// config apply — which replaces the sweep task wholesale.
struct SweepTarget {
    source_id: String,
    registry: crate::sources::PackageRegistry,
    ecosystem: &'static str,
    package: String,
}

/// Delay before the first sweep round.
///
/// Startup already runs outbox recovery and the first poll of every source;
/// adding third-party advisory traffic to that burst makes a cold start slower
/// and buys nothing — the backlog this sweep drains is static.
const SWEEP_START_DELAY: Duration = Duration::from_secs(60);

/// Resolve a watch to a sweep target, or `None` when it has no OSV coordinate
/// (Git forges, containers, feeds, CPAN — see [`crate::advisory`]).
fn sweep_target(watch: &Watch) -> Option<SweepTarget> {
    let provider = &watch.provider;
    let (registry, package) = crate::advisory::coordinate_from_source(
        provider.id(),
        provider.kind(),
        provider.display_name(),
    )?;
    let ecosystem = crate::advisory::Ecosystem::for_registry(registry)?;
    Some(SweepTarget {
        source_id: provider.id().to_owned(),
        registry,
        ecosystem: ecosystem.as_str(),
        package: package.to_owned(),
    })
}

/// Background advisory sweep: work through every watched package source's
/// never-checked versions so coverage does not depend on someone opening a page.
///
/// Returns `None` — no task at all — when the sweep is disabled or no watched
/// source has an OSV coordinate, so an instance that cannot use it does not
/// carry an idle task.
///
/// Delivery-time enrichment only ever covers versions that produced a
/// notification, and a baseline (first) poll notifies nothing. Without this,
/// everything a source was already publishing when it was first added stays
/// permanently unchecked unless a human visits its detail page.
#[must_use]
pub fn spawn_advisory_sweep(
    engine: Arc<Engine>,
    watches: &[Watch],
    shutdown: Arc<AtomicBool>,
) -> Option<tokio::task::JoinHandle<()>> {
    let interval = engine.advisories.sweep_interval()?;
    let batch = engine.advisories.sweep_batch();
    let targets: Vec<SweepTarget> = watches.iter().filter_map(sweep_target).collect();
    if targets.is_empty() {
        return None;
    }

    info!(
        sources = targets.len(),
        interval_secs = interval.as_secs(),
        batch,
        "background advisory sweep enabled"
    );

    Some(tokio::spawn(async move {
        if !sleep_interruptible(SWEEP_START_DELAY, &shutdown).await {
            return;
        }
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            let checked = sweep_advisories_once(&engine, &targets, batch, &shutdown).await;
            if checked > 0 {
                info!(checked, "advisory sweep round completed");
            }
            if !sleep_interruptible(interval, &shutdown).await {
                break;
            }
        }
    }))
}

/// Run one sweep round right now over `watches`, outside the background loop.
///
/// Returns how many versions got a verified answer recorded, `0` when the sweep
/// is disabled or nothing is left to check. Exists so the round is reachable
/// without waiting out [`SWEEP_START_DELAY`] — used by tests, and the hook a
/// manual "sweep now" control would call.
pub async fn sweep_advisories_now(engine: &Engine, watches: &[Watch]) -> usize {
    if engine.advisories.sweep_interval().is_none() {
        return 0;
    }
    let targets: Vec<SweepTarget> = watches.iter().filter_map(sweep_target).collect();
    let never = AtomicBool::new(false);
    sweep_advisories_once(engine, &targets, engine.advisories.sweep_batch(), &never).await
}

/// One sweep round: up to `batch` never-checked versions per target.
///
/// Returns how many versions got a verified answer recorded.
///
/// Sources are walked one at a time while each source's own batch runs
/// concurrently — that keeps peak in-flight requests at `batch` regardless of
/// how many sources are configured, instead of fanning out
/// `sources × batch` at a third-party API.
///
/// A round is naturally self-limiting during an OSV outage: once the breaker
/// opens, every remaining lookup returns unverified without touching the
/// network, so the round finishes fast and records nothing.
async fn sweep_advisories_once(
    engine: &Engine,
    targets: &[SweepTarget],
    batch: usize,
    shutdown: &AtomicBool,
) -> usize {
    let mut checked = 0usize;
    for target in targets {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let versions = match engine.store.unchecked_seen_versions(
            &target.source_id,
            target.ecosystem,
            &target.package,
            batch,
        ) {
            Ok(versions) => versions,
            Err(err) => {
                warn!(
                    source = %target.source_id,
                    error = %err,
                    "advisory sweep skipped a source — work-queue read failed"
                );
                continue;
            }
        };
        if versions.is_empty() {
            continue;
        }

        let results = join_all(versions.into_iter().map(|version| async move {
            let outcome = engine
                .advisories
                .lookup(target.registry, &target.package, &version)
                .await;
            (version, outcome)
        }))
        .await;

        for (version, outcome) in results {
            // Only a real OSV answer is recorded. Marking a failed or
            // breaker-skipped lookup as checked would retire that version from
            // every future round — permanently "clean" without ever asking.
            if !outcome.verified {
                continue;
            }
            crate::pipeline::persist_advisory_result(
                engine,
                target.ecosystem,
                &target.package,
                &version,
                &outcome.advisories,
            );
            checked += 1;
        }
    }
    checked
}

/// Spawn one Tokio task per watch. Caller is responsible for shutdown.
pub fn spawn_watchers(
    engine: Arc<Engine>,
    watches: Vec<Watch>,
    shutdown: Arc<AtomicBool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    if watches.is_empty() {
        warn!("no sources configured; nothing to watch");
    }

    watches
        .into_iter()
        .map(|watch| {
            let engine = Arc::clone(&engine);
            let shutdown = Arc::clone(&shutdown);
            tokio::spawn(async move { watch_loop(watch, engine, shutdown).await })
        })
        .collect()
}

async fn watch_loop(watch: Watch, engine: Arc<Engine>, shutdown: Arc<AtomicBool>) {
    let source_id = watch.provider.id().to_owned();

    if !watch.poll_on_startup
        && !sleep_interruptible(watch.interval + jitter_duration(watch.jitter), &shutdown).await
    {
        return;
    }

    let mut backoff = INITIAL_BACKOFF;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match poll_once(&engine, &watch, false).await {
            Ok(_) => backoff = INITIAL_BACKOFF,
            Err(err) => {
                warn!(source = %source_id, error = %err, backoff = ?backoff, "poll failed");
                if !sleep_interruptible(backoff, &shutdown).await {
                    break;
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        }
        if !sleep_interruptible(watch.interval + jitter_duration(watch.jitter), &shutdown).await {
            break;
        }
    }
}

/// Sleep up to `duration`, returning `false` as soon as shutdown is requested.
async fn sleep_interruptible(duration: Duration, shutdown: &AtomicBool) -> bool {
    let mut left = duration;
    while left > Duration::ZERO {
        if shutdown.load(Ordering::Relaxed) {
            return false;
        }
        let chunk = left.min(SHUTDOWN_POLL_CHUNK);
        tokio::time::sleep(chunk).await;
        left = left.saturating_sub(chunk);
    }
    !shutdown.load(Ordering::Relaxed)
}

fn jitter_duration(max: Duration) -> Duration {
    let secs = max.as_secs();
    if secs == 0 {
        return Duration::ZERO;
    }
    let chosen = rand::rng().random_range(0..=secs);
    Duration::from_secs(chosen)
}

/// Resolve when either SIGINT (Ctrl-C) or SIGTERM is received.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::engine::{Engine, PruneSettings};
    use crate::runtime::build_http_client;

    #[test]
    fn spawn_maintenance_should_be_none_when_disabled() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://a@b.c"]

            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test"
        "#,
        )
        .expect("parse");
        let http = build_http_client().expect("http");
        let Ok(engine) = Engine::open(&config, http) else {
            eprintln!("skipping scheduler test (postgres unavailable)");
            return;
        };
        assert!(!engine.prune_settings().periodic_enabled());
        let shutdown = Arc::new(AtomicBool::new(false));
        assert!(spawn_maintenance(engine, shutdown).is_none());
    }

    #[test]
    fn prune_settings_periodic_enabled_requires_interval_and_retention() {
        let enabled = PruneSettings {
            seen_after_days: 365,
            webhooks_after_days: 0,
            outbox_sent_after_days: 0,
            advisories_after_days: 0,
            interval_hours: 24,
        };
        assert!(enabled.periodic_enabled());

        let disabled = PruneSettings {
            seen_after_days: 0,
            webhooks_after_days: 0,
            outbox_sent_after_days: 0,
            advisories_after_days: 0,
            interval_hours: 24,
        };
        assert!(!disabled.periodic_enabled());

        // Advisories-only retention must gate periodic maintenance on its own,
        // same as the other three flags — it is not merely present on the
        // struct, it participates in the OR.
        let advisories_only = PruneSettings {
            advisories_after_days: 90,
            ..disabled
        };
        assert!(advisories_only.periodic_enabled());
    }

    #[tokio::test]
    async fn background_tasks_abort_all_should_not_panic() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://a@b.c"]

            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test"
        "#,
        )
        .expect("parse");
        let http = build_http_client().expect("http");
        let Ok(engine) = Engine::open(&config, http) else {
            eprintln!("skipping scheduler bg test (postgres unavailable)");
            return;
        };
        let tasks = start_background(engine, None).await;
        tasks.abort_all();
    }

    #[tokio::test]
    async fn sleep_interruptible_should_return_false_on_shutdown() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_set = Arc::clone(&flag);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag_set.store(true, Ordering::SeqCst);
        });
        let ok = sleep_interruptible(Duration::from_secs(5), &flag).await;
        assert!(!ok);
    }

    #[tokio::test]
    async fn graceful_shutdown_should_drain_without_panic() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://a@b.c"]

            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test"
        "#,
        )
        .expect("parse");
        let http = build_http_client().expect("http");
        let Ok(engine) = Engine::open(&config, http) else {
            eprintln!("skipping graceful shutdown test (postgres unavailable)");
            return;
        };
        let tasks = start_background(Arc::clone(&engine), None).await;
        tasks.graceful_shutdown(&engine).await;
    }
}
