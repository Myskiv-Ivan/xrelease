//! Small helpers shared by HTTP chat sinks (Slack / Telegram / eXpress / Novu).

use crate::error::NotifyError;

/// Reject blank required config fields with a consistent `Misconfigured` message.
pub(crate) fn require_non_empty(
    backend: &str,
    field: &str,
    value: &str,
) -> Result<(), NotifyError> {
    if value.trim().is_empty() {
        Err(NotifyError::Misconfigured(format!(
            "{backend} `{field}` must not be empty"
        )))
    } else {
        Ok(())
    }
}

/// Map Slack / Telegram-style `"ok": false` JSON bodies to [`NotifyError::Rejected`].
///
/// Both APIs often return HTTP 200 with a logical failure in the body. `error_key`
/// is `"error"` (Slack) or `"description"` (Telegram).
pub(crate) fn reject_if_ok_false(
    backend: &'static str,
    status: u16,
    body: &str,
    error_key: &str,
    label: &str,
) -> Result<(), NotifyError> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return Ok(());
    };
    if json.get("ok").and_then(serde_json::Value::as_bool) != Some(false) {
        return Ok(());
    }
    let detail = json
        .get(error_key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    Err(NotifyError::Rejected {
        backend,
        status,
        body: format!("{label} ok=false: {detail}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_non_empty_should_reject_blank() {
        assert!(require_non_empty("express", "token", "  ").is_err());
        assert!(require_non_empty("express", "token", "ok").is_ok());
    }

    #[test]
    fn reject_if_ok_false_should_map_logical_errors() {
        let err = reject_if_ok_false(
            "slack",
            200,
            r#"{"ok":false,"error":"channel_not_found"}"#,
            "error",
            "chat.postMessage",
        )
        .expect_err("ok=false");
        assert!(err.to_string().contains("channel_not_found"));
    }

    #[test]
    fn reject_if_ok_false_should_ignore_success() {
        assert!(
            reject_if_ok_false("telegram", 200, r#"{"ok":true}"#, "description", "send").is_ok()
        );
    }
}
