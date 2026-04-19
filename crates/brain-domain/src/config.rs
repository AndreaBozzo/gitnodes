//! Runtime configuration loaded from the process environment.
//!
//! These values are read once at server startup (see `brain-app/src/main.rs`),
//! then passed explicitly through constructors or provided via Leptos context.
//! No code outside of `main.rs` should call `std::env::var` for these keys.

use serde::{Deserialize, Serialize};

/// The GitHub repository the app reads from and writes to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetConfig {
    pub org: String,
    pub repo: String,
    pub branch: String,
}

impl TargetConfig {
    pub fn contents_url(&self, path: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            self.org, self.repo, path
        )
    }

    pub fn tree_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
            self.org, self.repo, self.branch
        )
    }

    pub fn raw_base(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            self.org, self.repo, self.branch
        )
    }

    pub fn blob_base(&self) -> String {
        format!(
            "https://github.com/{}/{}/blob/{}",
            self.org, self.repo, self.branch
        )
    }
}

/// User-facing branding copy (landing page title, access-denied messages).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrandConfig {
    /// Display name shown in the header, e.g. "Dritara Brain".
    pub name: String,
    /// Organisation label shown in access-denied copy, e.g. "Dritara-Digital".
    /// In practice this matches `TargetConfig::org` but kept separate to allow
    /// prettier display casing if ever needed.
    pub org_label: String,
}
