//! Low-overhead runtime configuration with backward-compatible env aliases.

use gitnodes_domain::{BrandConfig, TargetConfig, TargetRef};

pub const DEFAULT_TARGET_BRANCH: &str = "main";
pub const DEFAULT_BRAND_NAME: &str = "Brain UI";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetBootstrap {
    pub target: TargetConfig,
    /// True when the preferred single-variable repository locator was used.
    pub compact_locator: bool,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_with_legacy(primary: &str, legacy: &str) -> Option<String> {
    non_empty(std::env::var(primary).ok()).or_else(|| non_empty(std::env::var(legacy).ok()))
}

fn parse_repository(value: &str) -> Result<(String, String), String> {
    let Some((owner, repo)) = value.trim().split_once('/') else {
        return Err("TARGET_GITHUB_REPOSITORY must use the owner/repo format".into());
    };
    let owner = owner.trim();
    let repo = repo.trim();
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Err("TARGET_GITHUB_REPOSITORY must use the owner/repo format".into());
    }
    Ok((owner.to_string(), repo.to_string()))
}

pub fn target_from_values(
    repository: Option<String>,
    org: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
) -> Result<TargetBootstrap, String> {
    let repository = non_empty(repository);
    let compact_locator = repository.is_some();
    let (org, repo) = match repository {
        Some(repository) => parse_repository(&repository)?,
        None => (
            non_empty(org).ok_or_else(|| {
                "set TARGET_GITHUB_REPOSITORY=owner/repo (or TARGET_GITHUB_ORG)".to_string()
            })?,
            non_empty(repo).ok_or_else(|| {
                "set TARGET_GITHUB_REPOSITORY=owner/repo (or TARGET_GITHUB_REPO)".to_string()
            })?,
        ),
    };
    let target = TargetConfig {
        org,
        repo,
        branch: non_empty(branch).unwrap_or_else(|| DEFAULT_TARGET_BRANCH.to_string()),
    };
    TargetRef::from(&target)
        .validate()
        .map_err(|error| format!("invalid target repository configuration: {error}"))?;
    Ok(TargetBootstrap {
        target,
        compact_locator,
    })
}

pub fn target_from_env() -> Result<TargetBootstrap, String> {
    target_from_values(
        std::env::var("TARGET_GITHUB_REPOSITORY").ok(),
        env_with_legacy("TARGET_GITHUB_ORG", "GITHUB_ORG"),
        env_with_legacy("TARGET_GITHUB_REPO", "GITHUB_REPO"),
        env_with_legacy("TARGET_GITHUB_BRANCH", "GITHUB_BRANCH"),
    )
}

pub fn target_from_env_or_exit() -> TargetBootstrap {
    target_from_env().unwrap_or_else(|error| {
        tracing::error!(%error, "invalid runtime configuration");
        std::process::exit(1)
    })
}

pub fn brand_from_env(target: &TargetConfig) -> BrandConfig {
    BrandConfig {
        name: non_empty(std::env::var("BRAND_NAME").ok())
            .unwrap_or_else(|| DEFAULT_BRAND_NAME.to_string()),
        org_label: non_empty(std::env::var("BRAND_ORG_LABEL").ok())
            .unwrap_or_else(|| target.org.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_repository_defaults_to_main() {
        let bootstrap =
            target_from_values(Some(" acme / notes ".into()), None, None, None).unwrap();
        assert_eq!(
            bootstrap.target,
            TargetConfig {
                org: "acme".into(),
                repo: "notes".into(),
                branch: "main".into(),
            }
        );
        assert!(bootstrap.compact_locator);
    }

    #[test]
    fn legacy_split_values_remain_supported() {
        let bootstrap = target_from_values(
            None,
            Some("acme".into()),
            Some("notes".into()),
            Some("develop".into()),
        )
        .unwrap();
        assert_eq!(bootstrap.target.branch, "develop");
        assert!(!bootstrap.compact_locator);
    }

    #[test]
    fn compact_repository_rejects_extra_segments() {
        assert!(target_from_values(Some("acme/notes/extra".into()), None, None, None).is_err());
    }
}
