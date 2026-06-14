// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

pub mod access;
pub mod assets;
pub mod audit;
pub mod auth;
#[cfg(feature = "embed-assets")]
pub mod embedded;
pub mod health;
pub mod installation_token;
pub mod local;
pub mod pat;
pub mod pending_sync_job;
pub mod projection;
pub mod retention;
pub mod routing;
pub mod runtime_config;
pub mod session;
pub mod session_key;
pub mod sse;
pub mod target_registry;
pub mod webhook;
pub mod working_tree;
