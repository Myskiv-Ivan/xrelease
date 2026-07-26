//! Machine-readable description of what the desired-state config accepts.
//!
//! Exists so a UI can render **controls** — a source-kind `<select>`, a sink
//! picker, a team-tag multi-select, a schedule widget — instead of asking an
//! operator to hand-write TOML/YAML, and so those controls stay correct
//! without the frontend hardcoding option lists that silently drift from the
//! Rust enums (it already had 50 lines of them).
//!
//! Team tags and the running authoring mode are deployment data only the
//! server knows — that is why this schema is served, not hardcoded in the UI.
//!
//! Source and sink kind tables are drift-checked against serde's variant lists
//! (see tests below). Every sink kind is always available in the binary
//! (ADR-0001: all notifiers always on).

use serde::Serialize;

/// One selectable option with a human label.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaOption {
    /// Wire value written into the config document (`type = "github"`).
    pub value: &'static str,
    /// Human-facing label for a dropdown.
    pub label: &'static str,
}

/// A notification sink kind offered by this binary.
#[derive(Debug, Clone, Serialize)]
pub struct SinkOption {
    pub value: &'static str,
    pub label: &'static str,
    /// Always `true` — every documented sink kind is compiled into the binary.
    pub available: bool,
    /// Reserved for forward compatibility; always `None` (no cargo feature gates).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_feature: Option<&'static str>,
}

fn sink(value: &'static str, label: &'static str) -> SinkOption {
    SinkOption {
        value,
        label,
        available: true,
        requires_feature: None,
    }
}

/// Every `[[sources]]` `type` value the config accepts.
///
/// Kept in sync with the [`super::SourceConfig`] variants by
/// `source_kinds_should_match_serde_variants`, which asks serde itself for the
/// authoritative list rather than trusting this constant.
pub const SOURCE_KINDS: &[SchemaOption] = &[
    SchemaOption {
        value: "github",
        label: "GitHub",
    },
    SchemaOption {
        value: "codeberg",
        label: "Codeberg",
    },
    SchemaOption {
        value: "gitea",
        label: "Gitea / Forgejo",
    },
    SchemaOption {
        value: "gitlab",
        label: "GitLab",
    },
    SchemaOption {
        value: "bitbucket",
        label: "Bitbucket",
    },
    SchemaOption {
        value: "docker",
        label: "Docker Hub / OCI registry",
    },
    SchemaOption {
        value: "ghcr",
        label: "GitHub Container Registry",
    },
    SchemaOption {
        value: "quay",
        label: "Quay.io",
    },
    SchemaOption {
        value: "ecr",
        label: "AWS ECR Public",
    },
    SchemaOption {
        value: "feed",
        label: "RSS / Atom / JSON feed",
    },
    SchemaOption {
        value: "pypi",
        label: "PyPI",
    },
    SchemaOption {
        value: "npm",
        label: "npm",
    },
    SchemaOption {
        value: "yarn",
        label: "Yarn registry",
    },
    SchemaOption {
        value: "cargo",
        label: "crates.io",
    },
    SchemaOption {
        value: "maven",
        label: "Maven Central",
    },
    SchemaOption {
        value: "nuget",
        label: "NuGet",
    },
    SchemaOption {
        value: "hex",
        label: "Hex.pm",
    },
    SchemaOption {
        value: "rubygems",
        label: "RubyGems",
    },
    SchemaOption {
        value: "packagist",
        label: "Packagist",
    },
    SchemaOption {
        value: "cpan",
        label: "CPAN",
    },
    SchemaOption {
        value: "artifacthub",
        label: "Artifact Hub",
    },
];

/// SMTP transport modes (`[[notifiers]] type = "smtp"`, field `tls`).
pub const SMTP_TLS_MODES: &[SchemaOption] = &[
    SchemaOption {
        value: "starttls",
        label: "STARTTLS (587)",
    },
    SchemaOption {
        value: "tls",
        label: "Implicit TLS (465)",
    },
    SchemaOption {
        value: "plain",
        label: "Plaintext (no encryption)",
    },
];

/// HTTP methods a webhook sink may use.
pub const WEBHOOK_METHODS: &[SchemaOption] = &[
    SchemaOption {
        value: "POST",
        label: "POST",
    },
    SchemaOption {
        value: "PUT",
        label: "PUT",
    },
];

/// Notification sink kinds exposed to UI / OpenAPI (always all compiled kinds).
#[must_use]
pub fn sink_kinds() -> Vec<SinkOption> {
    vec![
        sink("apprise", "Apprise"),
        sink("webhook", "Webhook"),
        sink("express", "eXpress (BotX)"),
        sink("novu", "Novu"),
        sink("slack", "Slack"),
        sink("telegram", "Telegram"),
        sink("smtp", "SMTP e-mail"),
        sink("kafka", "Kafka"),
        sink("nats", "NATS"),
        sink("rabbitmq", "RabbitMQ"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// Drift guard: serde knows the authoritative variant list, so ask it.
    ///
    /// Deserializing an unknown `type` makes serde report `unknown variant
    /// ..., expected one of \`github\`, \`gitlab\`, ...` — comparing that set
    /// against [`SOURCE_KINDS`] catches both a variant added without updating
    /// this table and a stale entry left behind after one is removed.
    #[test]
    fn source_kinds_should_match_serde_variants() {
        let err = toml::from_str::<Config>(
            r#"
            [[sources]]
            type = "definitely-not-a-kind"
            repo = "x/y"
        "#,
        )
        .expect_err("unknown source kind must fail to parse")
        .to_string();

        let expected: std::collections::BTreeSet<String> = err
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| *token != "definitely-not-a-kind")
            .map(str::to_owned)
            .collect();
        assert!(
            !expected.is_empty(),
            "could not extract variants from serde error: {err}"
        );

        let listed: std::collections::BTreeSet<String> =
            SOURCE_KINDS.iter().map(|k| k.value.to_owned()).collect();
        assert_eq!(
            listed, expected,
            "SOURCE_KINDS is out of sync with the SourceConfig variants"
        );
    }

    #[test]
    fn sink_kinds_should_match_serde_variants() {
        let err = toml::from_str::<Config>(
            r#"
            [[notifiers]]
            type = "definitely-not-a-sink"
        "#,
        )
        .expect_err("unknown sink kind must fail to parse")
        .to_string();

        let expected: std::collections::BTreeSet<String> = err
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| *token != "definitely-not-a-sink")
            .map(str::to_owned)
            .collect();
        assert!(
            !expected.is_empty(),
            "could not extract variants from serde error: {err}"
        );

        let listed: std::collections::BTreeSet<String> =
            sink_kinds().iter().map(|k| k.value.to_owned()).collect();
        assert_eq!(
            listed, expected,
            "sink_kinds() is out of sync with the NotifierConfig variants"
        );
    }

    #[test]
    fn sink_kinds_should_mark_every_sink_available() {
        let kinds = sink_kinds();
        for kind in &kinds {
            assert!(
                kind.available,
                "{} must be available in every binary",
                kind.value
            );
            assert!(
                kind.requires_feature.is_none(),
                "{} must not require a cargo feature",
                kind.value
            );
        }
    }
}
