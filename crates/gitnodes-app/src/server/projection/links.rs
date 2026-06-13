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

use std::path::Path;

#[derive(Clone, Debug)]
pub(super) struct Backlink {
    pub(super) source_path: String,
    pub(super) target_path: String,
}

pub(super) fn resolve_link_path(from_dir: &Path, link: &str) -> Option<String> {
    let joined = from_dir.join(link);
    let mut parts: Vec<&str> = Vec::new();
    for component in joined.iter() {
        let segment = component.to_str()?;
        if segment == "." {
            continue;
        }
        if segment == ".." {
            parts.pop();
            continue;
        }
        parts.push(segment);
    }
    Some(parts.join("/"))
}
