//! `xrelease` binary entrypoint.
//!
//! Thin CLI shell over [`xrelease::runtime`]. All logic lives in the library crate.
//!
//! Sync-only commands (`health`, `sources`, `outbox-requeue`, offline `validate`)
//! must **not** enter a Tokio runtime: the sync `postgres` client embeds its own
//! runtime and panics with "Cannot start a runtime from within a runtime" when
//! called from `#[tokio::main]` (Docker/K8s health probes use `xrelease health`).

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use xrelease::config::{self, ConfigPaths};
use xrelease::runtime;

/// Self-hosted release watcher — poll upstreams, notify via Apprise.
///
/// Default mode is the backend (`serve`: poller + HTTP API + webhooks).
#[derive(Debug, Parser)]
#[command(name = "xrelease", version, about)]
struct Cli {
    /// Path to the infrastructure (bootstrap) config file.
    #[arg(short, long, env = "XRELEASE_CONFIG", default_value = "bootstrap.toml")]
    config: PathBuf,

    /// Application config (YAML or TOML). Defaults to `app/releases.yaml` when present.
    #[arg(short = 'a', long = "app", env = "XRELEASE_APP_CONFIG")]
    app_config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the backend: poller + HTTP API + webhooks (recommended for Docker/K8s).
    Serve,
    /// List configured sources (no network).
    Sources {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Verify the state database is reachable.
    Health,
    /// Requeue dead-letter notifications (exhausted retries) for another attempt.
    OutboxRequeue,
    /// Validate config without network I/O (CI / pre-deploy).
    /// With `--app`, validates the merged infra + app view; otherwise `--config` only.
    Validate {
        /// Emit machine-readable JSON on stdout.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
        /// Probe each source with a live upstream fetch (requires network).
        #[arg(long)]
        online: bool,
        /// Treat warnings as errors (recommended for GitOps CI).
        #[arg(long)]
        strict: bool,
        /// Online probe: only this source id (e.g. `github:org/repo`).
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = ConfigPaths::resolve(cli.config.clone(), cli.app_config.clone());

    // Offline `validate` is the one command explicitly meant to tolerate an
    // unreachable database ("no network I/O", CI / pre-deploy) — it alone
    // uses `load_for_cli`'s graceful DB-optional resolution.
    if let Some(Command::Validate {
        format,
        online: false,
        strict,
        source,
    }) = &cli.command
    {
        let (format, strict, source) = (*format, *strict, source.clone());
        let config = config::load_for_cli(&paths)?;
        xrelease::logging::init(&config.log.level);
        return run_validate_offline(&config, format, strict, source);
    }

    // Every remaining command needs a reachable database anyway
    // (`Runtime::new`/`Engine::open` fail hard without one, unlike offline
    // validate's graceful fallback above), so resolve config exactly ONCE
    // here and reuse the same `Runtime` everywhere downstream. Previously
    // `main` resolved config via `load_for_cli` just to read `log.level`,
    // then unconditionally discarded it — `run_sync`/`run_async` re-resolved
    // from scratch via `Runtime::new`, which itself opens a *third*,
    // throwaway `Store` (and re-applies the full idempotent schema DDL)
    // purely to read the config ledger before dropping it. That mattered
    // most for `health`: Docker/Kubernetes invoke it as a repeated
    // healthcheck (every 30-60s, for the container's whole lifetime), so the
    // redundant pool-opens repeated forever and could plausibly blow a tight
    // healthcheck timeout under real DB latency.
    // Logging is initialised from the *bootstrap* file (a cheap local read, no
    // database) rather than from the fully-resolved config, so the tracing
    // emitted during config resolution itself — notably which store the
    // desired state came from — is not silently dropped before a subscriber
    // exists. `[log]` is bootstrap-only anyway, so this reads the
    // same value the resolved config would carry.
    let bootstrap = config::load_infra_bootstrap(&paths)?;
    xrelease::logging::init(&bootstrap.log.level);

    let http = runtime::build_http_client()?;
    let rt = runtime::Runtime::new(paths, http)?;

    let command = cli.command;
    match &command {
        Some(Command::Health) | Some(Command::Sources { .. }) | Some(Command::OutboxRequeue) => {
            run_sync(command, &rt)
        }
        _ => {
            let tokio_rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            tokio_rt.block_on(run_async(command, &rt))
        }
    }
}

fn run_validate_offline(
    config: &config::Config,
    format: OutputFormat,
    strict: bool,
    source: Option<String>,
) -> anyhow::Result<()> {
    let opts = xrelease::validate::ValidateOptions {
        strict,
        online: false,
        source_filter: source,
    };
    let mut report = xrelease::validate::validate_full(config, &opts);
    if strict {
        report.apply_strict(true);
    }
    print_validation_report(&report, format);
    if !report.valid {
        std::process::exit(1);
    }
    Ok(())
}

fn run_sync(command: Option<Command>, rt: &runtime::Runtime) -> anyhow::Result<()> {
    match command {
        Some(Command::Health) => {
            rt.health()?;
            println!("ok");
        }
        Some(Command::Sources { format }) => {
            let summaries = rt.source_summaries()?;
            print_sources(&summaries, format);
        }
        Some(Command::OutboxRequeue) => {
            let revived = rt.requeue_dead_outbox()?;
            println!("requeued {revived} dead notification(s)");
        }
        other => unreachable!(
            "run_sync called with a command other than Health/Sources/OutboxRequeue: {other:?}"
        ),
    }
    Ok(())
}

async fn run_async(command: Option<Command>, rt: &runtime::Runtime) -> anyhow::Result<()> {
    if let Some(Command::Validate {
        format,
        online: true,
        strict,
        source,
    }) = &command
    {
        let opts = xrelease::validate::ValidateOptions {
            strict: *strict,
            online: true,
            source_filter: source.clone(),
        };
        let mut report = xrelease::validate::validate_full(rt.config(), &opts);
        let http = runtime::build_http_client()?;
        let probes = xrelease::validate::probe_sources(rt.config(), &http, &opts).await;
        xrelease::validate::merge_probes(&mut report, probes);
        if *strict {
            report.apply_strict(true);
        }
        print_validation_report(&report, *format);
        if !report.valid {
            std::process::exit(1);
        }
        return Ok(());
    }

    match command.unwrap_or(Command::Serve) {
        Command::Serve => {
            rt.serve().await?;
        }
        Command::Validate { .. }
        | Command::Health
        | Command::Sources { .. }
        | Command::OutboxRequeue => {
            unreachable!("sync commands handled in run_sync")
        }
    }

    Ok(())
}

fn print_sources(summaries: &[runtime::SourceSummary], format: OutputFormat) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(summaries) {
            Ok(json) => println!("{json}"),
            Err(err) => eprintln!("error: failed to serialize sources: {err}"),
        },
        OutputFormat::Text => {
            if summaries.is_empty() {
                println!("No sources configured.");
                return;
            }
            for s in summaries {
                println!(
                    "{} [{}] {} — every {}s (+{}s jitter)",
                    s.id, s.kind, s.display_name, s.interval_secs, s.jitter_secs
                );
            }
        }
    }
}

fn print_validation_report(report: &xrelease::validate::ValidationReport, format: OutputFormat) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(report) {
            Ok(json) => println!("{json}"),
            Err(err) => eprintln!("error: failed to serialize validation: {err}"),
        },
        OutputFormat::Text => {
            if report.valid {
                println!("config ok — {} source(s)", report.sources);
            } else {
                println!("config invalid — {} source(s)", report.sources);
            }
            for err in &report.errors {
                eprintln!("error: {err}");
            }
            for warn in &report.warnings {
                eprintln!("warning: {warn}");
            }
            if !report.probes.is_empty() {
                let ok = report.probes.iter().filter(|p| p.ok).count();
                println!("online probes: {ok}/{} ok", report.probes.len());
                for probe in &report.probes {
                    if probe.ok {
                        println!(
                            " probe {} [{}]: ok ({} releases{})",
                            probe.source_id,
                            probe.kind,
                            probe.releases.unwrap_or(0),
                            if probe.not_modified { ", 304" } else { "" }
                        );
                    } else {
                        eprintln!(
                            " probe {} [{}]: {}",
                            probe.source_id,
                            probe.kind,
                            probe.error.as_deref().unwrap_or("failed")
                        );
                    }
                }
            }
        }
    }
}
