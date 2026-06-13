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
