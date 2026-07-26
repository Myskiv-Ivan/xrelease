//! Application roles for UI / local auth / OIDC claim mapping.

use serde::{Deserialize, Serialize};

/// UI and API role ladder (`viewer` < `operator` < `admin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppRole {
    Viewer,
    Operator,
    Admin,
}

impl AppRole {
    /// Parse a role string; unknown values become [`None`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "viewer" => Some(Self::Viewer),
            "operator" => Some(Self::Operator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    /// Wire / DB value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }

    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Viewer => 1,
            Self::Operator => 2,
            Self::Admin => 3,
        }
    }

    /// Whether this role satisfies a route requiring at least `minimum`.
    #[must_use]
    pub(crate) const fn allows(self, minimum: Self) -> bool {
        self.rank() >= minimum.rank()
    }

    /// The higher-privilege of two roles.
    #[must_use]
    pub(crate) fn max(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }
}

/// A principal's resolved roles: one instance-wide role plus per-organization
/// grants. Populated from OIDC group claims — a bare alias
/// (`xrelease-admin`) grants the role globally; a scoped alias
/// (`xrelease-admin:platform`) grants it for that organization only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoles {
    pub global: AppRole,
    pub per_org: std::collections::BTreeMap<String, AppRole>,
}

impl ResolvedRoles {
    /// A single instance-wide role with no per-org grants (test helper; local
    /// users and the api key carry their role inline on the principal).
    #[cfg(test)]
    #[must_use]
    pub fn flat(role: AppRole) -> Self {
        Self {
            global: role,
            per_org: std::collections::BTreeMap::new(),
        }
    }

    /// Effective role for a target organization: the **higher** of the global
    /// role and any org-specific grant. `None` (whole-document routes) uses the
    /// global role. So a global viewer who is `admin` of `platform` is admin
    /// there and viewer elsewhere; a global admin is admin everywhere.
    #[must_use]
    pub fn for_org(&self, organization: Option<&str>) -> AppRole {
        match organization {
            None => self.global,
            Some(org) => self
                .per_org
                .get(org)
                .copied()
                .map_or(self.global, |scoped| scoped.max(self.global)),
        }
    }
}

impl std::fmt::Display for AppRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Map IdP role/group claim values onto an [`AppRole`].
#[must_use]
pub fn resolve_app_role(
    claimed: &[String],
    admin_aliases: &[String],
    operator_aliases: &[String],
    viewer_aliases: &[String],
    fallback: AppRole,
) -> AppRole {
    let normalized: std::collections::HashSet<String> = claimed
        .iter()
        .map(|role| role.to_ascii_lowercase())
        .collect();

    let mut resolved = fallback;
    for (app_role, aliases) in [
        (AppRole::Admin, admin_aliases),
        (AppRole::Operator, operator_aliases),
        (AppRole::Viewer, viewer_aliases),
    ] {
        if aliases
            .iter()
            .any(|alias| normalized.contains(&alias.to_ascii_lowercase()))
            && app_role.rank() >= resolved.rank()
        {
            resolved = app_role;
        }
    }
    resolved
}

/// Resolve a principal's global role plus per-organization grants from IdP
/// group claims. A bare alias (`xrelease-admin`) sets the global role; a scoped
/// alias (`xrelease-admin:platform`) grants that role for `platform` only.
#[must_use]
pub fn resolve_resolved_roles(
    claimed: &[String],
    admin_aliases: &[String],
    operator_aliases: &[String],
    viewer_aliases: &[String],
    fallback: AppRole,
) -> ResolvedRoles {
    let global = resolve_app_role(
        claimed,
        admin_aliases,
        operator_aliases,
        viewer_aliases,
        fallback,
    );

    let mut per_org: std::collections::BTreeMap<String, AppRole> =
        std::collections::BTreeMap::new();
    for group in claimed {
        let Some((alias, org)) = group.split_once(':') else {
            continue;
        };
        let org = org.trim();
        if org.is_empty() {
            continue;
        }
        if let Some(role) = role_for_alias(alias, admin_aliases, operator_aliases, viewer_aliases) {
            per_org
                .entry(org.to_owned())
                .and_modify(|current| *current = current.max(role))
                .or_insert(role);
        }
    }

    ResolvedRoles { global, per_org }
}

/// Map one (bare) alias onto its [`AppRole`], if any list contains it.
fn role_for_alias(
    alias: &str,
    admin_aliases: &[String],
    operator_aliases: &[String],
    viewer_aliases: &[String],
) -> Option<AppRole> {
    let alias = alias.trim().to_ascii_lowercase();
    let matches = |aliases: &[String]| aliases.iter().any(|a| a.to_ascii_lowercase() == alias);
    if matches(admin_aliases) {
        Some(AppRole::Admin)
    } else if matches(operator_aliases) {
        Some(AppRole::Operator)
    } else if matches(viewer_aliases) {
        Some(AppRole::Viewer)
    } else {
        None
    }
}

/// Read a nested JSON claim (`groups`, `realm_access.roles`, …) as string list.
#[must_use]
pub fn claim_strings(claims: &serde_json::Value, path: &str) -> Vec<String> {
    let mut current = claims;
    for key in path.split('.').filter(|part| !part.is_empty()) {
        match current.get(key) {
            Some(next) => current = next,
            None => return Vec::new(),
        }
    }

    let mut roles = Vec::new();
    if let Some(items) = current.as_array() {
        for item in items {
            if let Some(value) = item.as_str() {
                if !value.is_empty() {
                    roles.push(value.to_owned());
                }
            }
        }
    } else if let Some(value) = current.as_str() {
        if !value.is_empty() {
            roles.push(value.to_owned());
        }
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_app_role_should_prefer_highest_match() {
        let role = resolve_app_role(
            &["xrelease-operator".into(), "xrelease-admin".into()],
            &["xrelease-admin".into()],
            &["xrelease-operator".into()],
            &["xrelease-viewer".into()],
            AppRole::Viewer,
        );
        assert_eq!(role, AppRole::Admin);
    }

    #[test]
    fn resolve_app_role_should_use_fallback() {
        let role = resolve_app_role(
            &["other".into()],
            &["admin".into()],
            &["operator".into()],
            &["viewer".into()],
            AppRole::Viewer,
        );
        assert_eq!(role, AppRole::Viewer);
    }

    #[test]
    fn claim_strings_should_read_nested_array() {
        let claims = json!({ "realm_access": { "roles": ["admin", "viewer"] } });
        assert_eq!(
            claim_strings(&claims, "realm_access.roles"),
            vec!["admin", "viewer"]
        );
    }

    #[test]
    fn resolve_resolved_roles_should_parse_scoped_aliases() {
        let roles = resolve_resolved_roles(
            &[
                "xrelease-viewer".into(),
                "xrelease-admin:platform".into(),
                "xrelease-operator:security".into(),
            ],
            &["xrelease-admin".into()],
            &["xrelease-operator".into()],
            &["xrelease-viewer".into()],
            AppRole::Viewer,
        );
        assert_eq!(roles.global, AppRole::Viewer);
        assert_eq!(roles.per_org.get("platform"), Some(&AppRole::Admin));
        assert_eq!(roles.per_org.get("security"), Some(&AppRole::Operator));
        assert_eq!(roles.for_org(Some("platform")), AppRole::Admin);
        assert_eq!(roles.for_org(Some("other")), AppRole::Viewer);
        assert_eq!(roles.for_org(None), AppRole::Viewer);
    }

    #[test]
    fn resolve_resolved_roles_global_admin_wins_everywhere() {
        let roles = resolve_resolved_roles(
            &["xrelease-admin".into(), "xrelease-viewer:platform".into()],
            &["xrelease-admin".into()],
            &["xrelease-operator".into()],
            &["xrelease-viewer".into()],
            AppRole::Viewer,
        );
        assert_eq!(roles.global, AppRole::Admin);
        assert_eq!(roles.for_org(Some("platform")), AppRole::Admin);
    }
}
