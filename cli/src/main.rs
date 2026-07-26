//! `xrctl` — remote management CLI for a running `xrelease serve`.
//!
//! A thin HTTP client (see [`client`]): it manages a *remote* instance over
//! the management API and deliberately touches no local config file or
//! database, so it can run from a CI runner or a laptop with no PostgreSQL
//! credentials. Config authoring stays whole-document — `apply` submits a
//! complete desired-state file; there is no field-level "add a source" that
//! would rewrite YAML.
//!
//! Ship independently of the backend: binary release archives, or the lean
//! container image built from `docker/Dockerfile.cli` (`xrelease-cli` on GHCR).

mod client;

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};

use client::{content_type_for_path, ApiClient, IfMatch, DEFAULT_API_URL};

/// Manage a running xrelease instance over its HTTP API.
#[derive(Debug, Parser)]
#[command(name = "xrctl", version, about)]
struct Cli {
    /// Management API base URL of the target instance.
    ///
    /// Default `http://127.0.0.1:8080` matches `[api].listen`. Docker Compose
    /// UI stack: use `http://127.0.0.1:3000` (nginx proxies `/api`; backend
    /// `:8080` is not published on the host).
    ///
    /// Pass explicitly — xrctl does not read connection settings from the
    /// environment (server-side `XRELEASE_API_KEY` still configures the backend).
    #[arg(long, default_value = DEFAULT_API_URL, global = true)]
    api_url: String,
    /// Bearer token matching `[api].api_key` on the target instance.
    #[arg(long, global = true)]
    api_key: Option<String>,
    /// Scope to one organization (`[[organizations]].id`).
    ///
    /// Config commands use `/api/v1/organizations/{id}/config/…`; `sources` and
    /// `outbox` pass `?organization=`.
    #[arg(long, global = true)]
    organization: Option<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, global = true)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the effective running config (secrets redacted).
    Show,
    /// Show what the desired-state config accepts (source kinds, sinks, presets).
    Schema,
    /// List config apply/reject history from the audit ledger.
    History {
        /// Rows to fetch (server clamps to 1..200).
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Dry-run a desired-state document without applying it (exits non-zero if invalid).
    Validate {
        /// Path to the desired-state document (YAML or TOML).
        file: PathBuf,
    },
    /// Apply a desired-state document to the running instance.
    Apply {
        /// Path to the desired-state document (YAML or TOML).
        file: PathBuf,
        /// Optimistic concurrency: `auto` (read the current ETag first and
        /// require it — safe against a concurrent editor), `none`
        /// (unconditional, for CI pushing committed state), or an explicit sha.
        #[arg(long, default_value = "auto")]
        if_match: String,
        /// Audit label recorded with the revision (e.g. a git SHA).
        #[arg(long)]
        label: Option<String>,
    },
    /// Re-apply the previous applied revision.
    Rollback,
    /// Ask the instance to re-read its desired state from its own authority
    /// (single app file, or every [[organizations]] document).
    Reload,
    /// List the organization catalogue ([[organizations]] in bootstrap.toml).
    Organizations,
    /// Process overview (uptime, outbox depth, open breakers).
    Status,
    /// Configured sources with live runtime state.
    Sources,
    /// Pending/failed notification outbox entries.
    Outbox,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

// A one-shot CLI issues at most two sequential requests, so the multi-thread
// scheduler and its worker pool are dead weight — the current-thread runtime is
// leaner and starts faster.
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = ApiClient::new(&cli.api_url, cli.api_key, cli.organization)?;
    let format = cli.format;

    match cli.command {
        Command::Show => print(&client.config_show().await?, format, print_config_summary),
        Command::Organizations => {
            print(&client.organizations().await?, format, print_organizations);
        }
        Command::Schema => print(&client.config_schema().await?, format, print_config_summary),
        Command::History { limit } => {
            print(
                &client.config_history(limit).await?,
                format,
                print_config_summary,
            );
        }
        Command::Status => print(&client.status().await?, format, print_status_summary),
        Command::Sources => print(&client.sources().await?, format, print_json_pretty),
        Command::Outbox => print(&client.outbox().await?, format, print_json_pretty),
        Command::Rollback => print(
            &client.config_rollback().await?,
            format,
            print_config_summary,
        ),
        Command::Reload => print(&client.config_reload().await?, format, print_config_summary),
        Command::Apply {
            file,
            if_match,
            label,
        } => {
            let document = read_document(&file)?;
            let content_type = content_type_for_path(&file);
            let precondition = IfMatch::from_flag(&if_match);
            let response = client
                .config_apply(document, content_type, &precondition, label.as_deref())
                .await?;
            print(&response, format, print_config_summary);
        }
        Command::Validate { file } => {
            let document = read_document(&file)?;
            let content_type = content_type_for_path(&file);
            let response = client.config_validate(document, content_type).await?;
            // A dry-run that reports `valid: false` is a *successful* request
            // but a failed check — exit non-zero so CI treats it as a gate.
            let valid = response
                .get("valid")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            print(&response, format, print_validation_summary);
            if !valid {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn read_document(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("reading desired-state document {}", path.display()))
}

fn print(value: &serde_json::Value, format: OutputFormat, text: impl Fn(&serde_json::Value)) {
    match format {
        OutputFormat::Json => print_json_pretty(value),
        OutputFormat::Text => text(value),
    }
}

fn print_json_pretty(value: &serde_json::Value) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("error: failed to serialize response: {err}"),
    }
}

/// Compact overview for `xrctl status` (full JSON with `--format json`).
fn print_status_summary(value: &serde_json::Value) {
    let num = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .or_else(|| {
                value
                    .get(key)
                    .and_then(serde_json::Value::as_i64)
                    .map(|n| n as u64)
            })
            .unwrap_or(0)
    };
    if let Some(version) = value.get("version").and_then(serde_json::Value::as_str) {
        println!("version: {version}");
    }
    println!("uptime: {}s", num("uptime_secs"));
    println!("sources: {}", num("sources_configured"));
    println!(
        "outbox: pending+retry {} dead {} deferred {}",
        num("outbox_pending"),
        num("outbox_dead"),
        num("outbox_deferred")
    );
    if let Some(ok) = value
        .get("database_ok")
        .and_then(serde_json::Value::as_bool)
    {
        println!("database: {}", if ok { "ok" } else { "down" });
    }
    if let Some(breakers) = value.get("breakers").and_then(serde_json::Value::as_array) {
        let open: Vec<&str> = breakers
            .iter()
            .filter(|b| b.get("open").and_then(serde_json::Value::as_bool) == Some(true))
            .filter_map(|b| b.get("label").and_then(serde_json::Value::as_str))
            .collect();
        if open.is_empty() {
            println!("breakers open: none ({} tracked)", breakers.len());
        } else {
            println!("breakers open: {}", open.join(", "));
        }
    }
}

/// Text rendering for `xrctl organizations`.
fn print_organizations(value: &serde_json::Value) {
    let Some(organizations) = value
        .get("organizations")
        .and_then(serde_json::Value::as_array)
    else {
        print_json_pretty(value);
        return;
    };
    if organizations.is_empty() {
        println!("no [[organizations]] declared (single-document instance)");
        return;
    }
    for org in organizations {
        println!(
            "{:<20} {:<30} {} source(s)",
            org.get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
            org.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
            org.get("source_count")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
        );
    }
    if let Some(source) = value
        .get("config_source")
        .and_then(serde_json::Value::as_str)
    {
        println!("config source: {source}");
    }
}

fn print_validation_summary(value: &serde_json::Value) {
    let valid = value
        .get("valid")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    println!("{}", if valid { "config ok" } else { "config invalid" });
    for key in ["errors", "warnings"] {
        if let Some(items) = value
            .pointer(&format!("/report/{key}"))
            .and_then(serde_json::Value::as_array)
        {
            for item in items.iter().filter_map(serde_json::Value::as_str) {
                let label = if key == "errors" { "error" } else { "warning" };
                eprintln!("{label}: {item}");
            }
        }
    }
}

/// Human-readable rendering for whichever config response shape came back.
///
/// One printer for every config command: they return different documents, so
/// it renders the fields each one actually has rather than switching on the
/// originating command — which keeps this from drifting when a field is added.
fn print_config_summary(value: &serde_json::Value) {
    let str_field = |key: &str| value.get(key).and_then(serde_json::Value::as_str);

    if let Some(revisions) = value.get("revisions").and_then(serde_json::Value::as_array) {
        let total = value
            .get("total")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        println!("{} revision(s) of {total}", revisions.len());
        for revision in revisions {
            println!(
                " #{:<5} {:<9} {} {}{}{}",
                revision
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                revision
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?"),
                revision
                    .get("content_sha256")
                    .and_then(serde_json::Value::as_str)
                    .map_or("-", |sha| sha.get(..12).unwrap_or(sha)),
                revision
                    .get("applied_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                revision
                    .get("applied_by")
                    .and_then(serde_json::Value::as_str)
                    .map(|who| format!(" by {who}"))
                    .unwrap_or_default(),
                // Global multi-org history mixes streams; label each row.
                revision
                    .get("organization_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|org| format!(" [{org}]"))
                    .unwrap_or_default()
            );
        }
        return;
    }

    if let Some(kinds) = value
        .get("source_kinds")
        .and_then(serde_json::Value::as_array)
    {
        let kind_names = kinds
            .iter()
            .filter_map(|item| {
                item.get("value")
                    .or_else(|| item.get("name"))
                    .and_then(serde_json::Value::as_str)
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("source kinds ({}): {kind_names}", kinds.len());
        if let Some(sinks) = value
            .get("sink_kinds")
            .and_then(serde_json::Value::as_array)
        {
            let available: Vec<&str> = sinks
                .iter()
                .filter(|sink| {
                    sink.get("available").and_then(serde_json::Value::as_bool) == Some(true)
                })
                .filter_map(|sink| sink.get("value").and_then(serde_json::Value::as_str))
                .collect();
            println!("sinks available in this build: {}", available.join(", "));
        }
        if let Some(presets) = value
            .get("preset_names")
            .and_then(serde_json::Value::as_array)
        {
            let names: Vec<&str> = presets
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect();
            println!("presets: {}", names.join(", "));
        }
        if let Some(tags) = value.get("team_tags").and_then(serde_json::Value::as_array) {
            let names: Vec<&str> = tags.iter().filter_map(serde_json::Value::as_str).collect();
            println!("team tags: {}", names.join(", "));
        }
        return;
    }

    // apply / rollback / reload outcome
    if let Some(applied) = value.get("applied").and_then(serde_json::Value::as_bool) {
        let sha = str_field("content_sha256").unwrap_or("");
        println!(
            "{} (sha {})",
            if applied { "applied" } else { "unchanged" },
            sha.get(..12).unwrap_or(sha)
        );
        for (key, label) in [
            ("sources_added", "added"),
            ("sources_removed", "removed"),
            ("sources_changed", "changed"),
            ("warnings", "warning"),
        ] {
            if let Some(items) = value.get(key).and_then(serde_json::Value::as_array) {
                for item in items.iter().filter_map(serde_json::Value::as_str) {
                    println!(" {label}: {item}");
                }
            }
        }
        return;
    }

    // `show` (global ConfigView) and per-org OrganizationConfigView share
    // desired_source / desired_content / content_sha256; org responses also
    // carry organization + app_path (front / OpenAPI), not app_config_path.
    if let Some(source) = str_field("desired_source") {
        if let Some(org) = str_field("organization") {
            println!("organization: {org}");
        }
        if let Some(name) = str_field("name") {
            println!("name: {name}");
        }
        println!("desired source: {source}");
        if let Some(sha) = str_field("content_sha256") {
            println!("revision sha: {}", sha.get(..16).unwrap_or(sha));
        }
        if let Some(label) = str_field("revision_label") {
            println!("revision label: {label}");
        }
        if let Some(path) = str_field("app_config_path").or_else(|| str_field("app_path")) {
            println!("app config: {path}");
        }
        if let Some(content) = str_field("desired_content") {
            println!("---\n{content}");
        }
        return;
    }

    print_json_pretty(value);
}
