// Copyright 2026 Andrea Bozzo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use serde::{Deserialize, Serialize};

/// Provider-agnostic operational item tracked by Brain.
///
/// This is intentionally *not* a GitHub/GitLab issue mirror. The domain keeps
/// its own stable identity (`brain_id`) and can optionally bind to an external
/// tracker item when the current forge supports it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkItem {
    pub brain_id: String,
    pub kind: WorkItemKind,
    pub title: String,
    pub state: WorkItemState,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_binding: Option<ExternalWorkItemBinding>,
    #[serde(default)]
    pub system_of_record: WorkItemSystemOfRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum WorkItemKind {
    Task,
    Discussion,
    Decision,
    Incident,
    Change,
    Quote,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum WorkItemState {
    Backlog,
    Todo,
    InProgress,
    Blocked,
    Done,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WorkItemSystemOfRecord {
    #[default]
    Brain,
    External,
    Split,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalWorkItemBinding {
    pub system: ExternalWorkItemSystem,
    /// Project/repository/group identifier as understood by the external
    /// system. Stored as a string to keep GitHub/GitLab/Gitea compatible.
    pub project: String,
    /// Human-facing issue/item number or key (`123`, `ABC-42`, ...).
    pub item_key: String,
    /// Opaque provider id when available. Optional because some systems expose
    /// only a local IID/key in their public APIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalWorkItemSystem {
    Github,
    Gitlab,
    Gitea,
    Forgejo,
    Custom,
}

impl WorkItem {
    pub fn is_bound(&self) -> bool {
        self.external_binding.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_preserves_binding() {
        let item = WorkItem {
            brain_id: "wi_123".into(),
            kind: WorkItemKind::Task,
            title: "Stabilize webhook sync".into(),
            state: WorkItemState::InProgress,
            labels: vec!["brain:task".into()],
            assignees: vec!["andrea".into()],
            content_path: Some("tasks/stabilize-webhook-sync.md".into()),
            external_binding: Some(ExternalWorkItemBinding {
                system: ExternalWorkItemSystem::Github,
                project: "AndreaBozzo/Brain_UI".into(),
                item_key: "123".into(),
                provider_id: Some("I_kwDO...".into()),
                url: Some("https://github.com/AndreaBozzo/Brain_UI/issues/123".into()),
            }),
            system_of_record: WorkItemSystemOfRecord::Split,
        };

        let json = serde_json::to_string(&item).unwrap();
        let parsed: WorkItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
        assert!(parsed.is_bound());
    }
}
