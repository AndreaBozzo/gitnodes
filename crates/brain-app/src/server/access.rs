//! Live repository authorization shared by server functions and Axum handlers.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use brain_domain::{BrainError, TargetConfig};
use brain_storage::{GithubStorage, RepositoryPermissions};
use sha2::{Digest, Sha256};

const PERMISSION_CACHE_TTL: Duration = Duration::from_secs(15);
const MAX_PERMISSION_CACHE_ENTRIES: usize = 2048;

#[derive(Clone, PartialEq, Eq, Hash)]
struct PermissionCacheKey {
    token_fingerprint: [u8; 32],
    owner: String,
    repo: String,
}

#[derive(Clone)]
struct PermissionCacheEntry {
    permissions: RepositoryPermissions,
    fetched_at: Instant,
}

static PERMISSION_CACHE: OnceLock<Mutex<HashMap<PermissionCacheKey, PermissionCacheEntry>>> =
    OnceLock::new();

fn permission_cache() -> &'static Mutex<HashMap<PermissionCacheKey, PermissionCacheEntry>> {
    PERMISSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(token: &str, target: &TargetConfig) -> PermissionCacheKey {
    PermissionCacheKey {
        token_fingerprint: Sha256::digest(token.as_bytes()).into(),
        owner: target.org.clone(),
        repo: target.repo.clone(),
    }
}

fn cached_permissions(token: &str, target: &TargetConfig) -> Option<RepositoryPermissions> {
    let key = cache_key(token, target);
    let mut cache = permission_cache()
        .lock()
        .expect("permission cache poisoned");
    let entry = cache.get(&key)?;
    if entry.fetched_at.elapsed() <= PERMISSION_CACHE_TTL {
        return Some(entry.permissions.clone());
    }
    cache.remove(&key);
    None
}

fn store_permissions(token: &str, target: &TargetConfig, permissions: RepositoryPermissions) {
    let key = cache_key(token, target);
    let mut cache = permission_cache()
        .lock()
        .expect("permission cache poisoned");
    cache.retain(|_, entry| entry.fetched_at.elapsed() <= PERMISSION_CACHE_TTL);
    if cache.len() >= MAX_PERMISSION_CACHE_ENTRIES
        && let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at)
            .map(|(key, _)| key.clone())
    {
        cache.remove(&oldest);
    }
    cache.insert(
        key,
        PermissionCacheEntry {
            permissions,
            fetched_at: Instant::now(),
        },
    );
}

pub async fn repository_permissions(
    storage: &GithubStorage,
    token: &str,
) -> Result<RepositoryPermissions, BrainError> {
    if let Some(permissions) = cached_permissions(token, storage.target()) {
        return Ok(permissions);
    }
    let permissions = storage.repository_permissions(token).await?;
    store_permissions(token, storage.target(), permissions.clone());
    Ok(permissions)
}

pub fn ensure_read(
    target: &TargetConfig,
    permissions: &RepositoryPermissions,
) -> Result<(), BrainError> {
    if permissions.pull {
        Ok(())
    } else {
        Err(BrainError::permission_denied(format!(
            "read access on {}/{} is required",
            target.org, target.repo
        )))
    }
}

pub fn ensure_admin(
    target: &TargetConfig,
    permissions: &RepositoryPermissions,
) -> Result<(), BrainError> {
    if permissions.admin || permissions.maintain {
        Ok(())
    } else {
        Err(BrainError::permission_denied(format!(
            "admin or maintain access on {}/{} is required",
            target.org, target.repo
        )))
    }
}

pub fn ensure_write(
    target: &TargetConfig,
    permissions: &RepositoryPermissions,
) -> Result<(), BrainError> {
    if permissions.push {
        Ok(())
    } else {
        Err(BrainError::permission_denied(format!(
            "write access on {}/{} is required",
            target.org, target.repo
        )))
    }
}

pub async fn require_read(
    storage: &GithubStorage,
    token: &str,
) -> Result<RepositoryPermissions, BrainError> {
    let permissions = repository_permissions(storage, token).await?;
    ensure_read(storage.target(), &permissions)?;
    Ok(permissions)
}

pub async fn require_admin(
    storage: &GithubStorage,
    token: &str,
) -> Result<RepositoryPermissions, BrainError> {
    let permissions = repository_permissions(storage, token).await?;
    ensure_admin(storage.target(), &permissions)?;
    Ok(permissions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> TargetConfig {
        TargetConfig {
            org: "octocat".into(),
            repo: "brain".into(),
            branch: "main".into(),
        }
    }

    #[test]
    fn read_requires_pull_permission() {
        assert!(
            ensure_read(
                &target(),
                &RepositoryPermissions {
                    pull: true,
                    ..Default::default()
                }
            )
            .is_ok()
        );
        assert!(matches!(
            ensure_read(&target(), &RepositoryPermissions::default()),
            Err(BrainError::PermissionDenied(_))
        ));
    }

    #[test]
    fn admin_accepts_admin_or_maintain() {
        for permissions in [
            RepositoryPermissions {
                admin: true,
                ..Default::default()
            },
            RepositoryPermissions {
                maintain: true,
                ..Default::default()
            },
        ] {
            assert!(ensure_admin(&target(), &permissions).is_ok());
        }
        assert!(matches!(
            ensure_admin(
                &target(),
                &RepositoryPermissions {
                    push: true,
                    ..Default::default()
                }
            ),
            Err(BrainError::PermissionDenied(_))
        ));
    }

    #[test]
    fn direct_write_requires_push_permission() {
        assert!(
            ensure_write(
                &target(),
                &RepositoryPermissions {
                    push: true,
                    ..Default::default()
                }
            )
            .is_ok()
        );
        assert!(matches!(
            ensure_write(
                &target(),
                &RepositoryPermissions {
                    pull: true,
                    ..Default::default()
                }
            ),
            Err(BrainError::PermissionDenied(_))
        ));
    }

    #[test]
    fn cache_key_isolated_by_token_and_target() {
        let a = cache_key("token-a", &target());
        let b = cache_key("token-b", &target());
        let other_target = TargetConfig {
            repo: "other".into(),
            ..target()
        };
        let c = cache_key("token-a", &other_target);
        assert!(a != b);
        assert!(a != c);
    }

    #[test]
    fn cache_key_reuses_repo_permissions_across_branches() {
        let a = cache_key("token-a", &target());
        let b = cache_key(
            "token-a",
            &TargetConfig {
                branch: "develop".into(),
                ..target()
            },
        );
        assert!(a == b);
    }
}
