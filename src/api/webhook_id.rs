//! Extract stable webhook delivery ids for idempotency.

use axum::http::HeaderMap;

/// Read a forge-specific delivery / idempotency header.
#[must_use]
pub fn delivery_id(headers: &HeaderMap, kind: &str) -> Option<String> {
    let id = match kind {
        "github" => header(headers, "x-github-delivery"),
        "gitlab" => header(headers, "x-gitlab-event-uuid")
            .or_else(|| header(headers, "x-gitlab-webhook-uuid")),
        "gitea" => {
            header(headers, "x-gitea-delivery").or_else(|| header(headers, "x-gitea-event-id"))
        }
        "generic" => {
            header(headers, "idempotency-key").or_else(|| header(headers, "x-idempotency-key"))
        }
        "bitbucket" => header(headers, "x-request-uuid"),
        "docker" => {
            header(headers, "idempotency-key").or_else(|| header(headers, "x-idempotency-key"))
        }
        _ => None,
    }?;
    if id.is_empty() {
        None
    } else {
        Some(format!("{kind}:{id}"))
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn delivery_id_should_prefix_kind() {
        let mut headers = HeaderMap::new();
        headers.insert("x-github-delivery", HeaderValue::from_static("abc-123"));
        assert_eq!(
            delivery_id(&headers, "github"),
            Some("github:abc-123".into())
        );
    }
}
