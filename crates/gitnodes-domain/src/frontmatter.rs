/// Split a raw markdown file with optional YAML frontmatter into (front, body).
/// Returns `("", raw)` if no frontmatter is present.
pub fn split_frontmatter(raw: &str) -> (&str, &str) {
    let Some(rest) = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
    else {
        return ("", raw);
    };
    let Some(end) = rest.find("\n---") else {
        return ("", raw);
    };
    let front = &rest[..end];
    let after = &rest[end..];
    let body = after
        .strip_prefix("\n---\n")
        .or_else(|| after.strip_prefix("\n---\r\n"))
        .unwrap_or("");
    (front, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter() {
        let (f, b) = split_frontmatter("# hi\nbody");
        assert_eq!(f, "");
        assert_eq!(b, "# hi\nbody");
    }

    #[test]
    fn basic_frontmatter() {
        let raw = "---\ntype: concept\ntopic: Foo\n---\nbody here";
        let (f, b) = split_frontmatter(raw);
        assert!(f.contains("type: concept"));
        assert_eq!(b, "body here");
    }

    #[test]
    fn crlf_frontmatter() {
        let raw = "---\r\ntype: adr\r\n---\r\nbody";
        let (f, b) = split_frontmatter(raw);
        assert!(f.contains("type: adr"));
        assert_eq!(b, "body");
    }

    #[test]
    fn unterminated_frontmatter_returns_raw() {
        let raw = "---\ntype: concept\nno-close";
        let (f, b) = split_frontmatter(raw);
        assert_eq!(f, "");
        assert_eq!(b, raw);
    }
}
