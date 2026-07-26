//! Cron-gated notification delivery schedule.

use chrono::{DateTime, Utc};

/// Cron-gated delivery window for a watch's notifications.
///
/// Cron defines *moments*: a deferred notification is held in the outbox with
/// `deliver_after = next matching moment` and delivered by the background
/// flush once that moment passes. Expressions are evaluated in **UTC**.
#[derive(Debug, Clone)]
pub struct NotifySchedule {
    /// Expression as written in config (for display in `sources` / UI).
    expr: String,
    schedule: cron::Schedule,
}

impl NotifySchedule {
    /// Parse a crontab expression.
    ///
    /// Accepts the familiar 5-field crontab form (`min hour day month dow`) by
    /// normalizing it to the seconds-first 6-field form the `cron` crate
    /// expects; 6- and 7-field forms pass through unchanged.
    ///
    /// # Errors
    /// Returns a human-readable message when the expression does not parse.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let trimmed = expr.trim();
        let normalized = if trimmed.split_whitespace().count() == 5 {
            format!("0 {trimmed}")
        } else {
            trimmed.to_owned()
        };
        let schedule = normalized
            .parse::<cron::Schedule>()
            .map_err(|err| format!("invalid cron expression `{expr}`: {err}"))?;
        Ok(Self {
            expr: trimmed.to_owned(),
            schedule,
        })
    }

    /// Expression as configured (5-field form is preserved for display).
    #[must_use]
    pub fn expr(&self) -> &str {
        &self.expr
    }

    /// Next delivery moment strictly after `now` (UTC).
    #[must_use]
    pub fn next_after(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.schedule.after(&now).next()
    }
}
