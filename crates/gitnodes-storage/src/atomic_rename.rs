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

//! Backwards-compatible entry points for the pre-transaction rename API.
//!
//! The implementation now lives in [`crate::git_transaction`], where save,
//! delete, and rename all share the same Git Data API transaction pipeline.

pub use crate::git_transaction::{BackoffPolicy, RenameMutation, RenameOutcome, run};
