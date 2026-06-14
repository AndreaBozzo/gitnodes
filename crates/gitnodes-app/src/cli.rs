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

//! `gitnodes` subcommands for first-run setup.
//!
//! The default action is to run the server (`serve`). `init` scaffolds a starter
//! brain so a new user has a repo of markdown to point GitNodes at — the empty
//! repo is otherwise the first wall they hit.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Starter brain payload, embedded from `examples/starter-brain/` so the
/// scaffold and the published example stay one source of truth (and the config
/// is covered by `gitnodes-domain`'s parse-validity test).
const STARTER_FILES: &[(&str, &str)] = &[
    (
        ".gitnodes.yml",
        include_str!("../../../examples/starter-brain/.gitnodes.yml"),
    ),
    (
        "concepts/knowledge-graph.md",
        include_str!("../../../examples/starter-brain/concepts/knowledge-graph.md"),
    ),
    (
        "concepts/markdown-frontmatter.md",
        include_str!("../../../examples/starter-brain/concepts/markdown-frontmatter.md"),
    ),
    (
        "adrs/0001-git-as-source-of-truth.md",
        include_str!("../../../examples/starter-brain/adrs/0001-git-as-source-of-truth.md"),
    ),
    (
        "projects/trial-run.md",
        include_str!("../../../examples/starter-brain/projects/trial-run.md"),
    ),
];

const GITIGNORE_HEADER: &str =
    "# GitNodes runtime artifacts - never commit secrets or the local DB";
const GITIGNORE_ENTRIES: &[&str] = &[".env", "/data/"];

/// One-line usage for `gitnodes help` and unknown commands.
pub fn print_usage() {
    eprintln!(
        "gitnodes — a knowledge graph over a Git repo of markdown\n\
\n\
USAGE:\n    \
gitnodes <command>\n\
\n\
COMMANDS:\n    \
serve [dir]   Run the server for a local Git checkout (default: current dir).\n    \
preview [dir] Browse a local folder of markdown in the graph UI — no GitHub,\n                  \
no login, read-only (default: current dir).\n    \
mcp [dir]     Serve read-only local knowledge tools to AI agents over stdio.\n    \
init [dir]    Scaffold a starter brain (.gitnodes.yml + sample notes + AGENTS.md).\n    \
agents [dir]  (Re)generate AGENTS.md from .gitnodes.yml so coding agents know\n                  \
the conventions of this knowledge base.\n    \
help          Show this message.\n"
    );
}

/// Render an `AGENTS.md` from a config, teaching any coding agent (Claude Code,
/// Codex, Cursor, …) the conventions of this specific knowledge base. Generated
/// from `.gitnodes.yml` so it always matches the live taxonomy.
pub fn render_agents_md(cfg: &gitnodes_domain::BrainConfig, root: &Path) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();

    // Absolute, forward-slashed path to this brain so the setup commands below
    // are copy-paste-ready. Forward slashes keep the JSON valid and work on
    // Windows too; `absolute` avoids touching the filesystem or the `\\?\` prefix.
    let repo_path = std::path::absolute(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");

    s.push_str(
        "# AGENTS.md\n\n\
This repository is a **GitNodes knowledge base**: a graph of markdown notes that \
humans and AI agents both read and edit. Git is the source of truth — every note is a \
plain markdown file with a YAML frontmatter block, and edits are ordinary commits or \
pull requests.\n\n\
> Generated from `.gitnodes.yml` by `gitnodes agents`. Re-run it after changing the config.\n\n",
    );

    s.push_str(
        "## Node types\n\n\
Each note declares a `type:` in its frontmatter; that decides which folder it belongs in \
and how it is styled.\n\n",
    );
    for t in &cfg.node_types {
        // Skip virtual types (e.g. `tag`) that have no directory of their own.
        if t.directory.is_empty() {
            continue;
        }
        let _ = write!(s, "- **{}** → `{}/`", t.name, t.directory);
        if let Some(title_key) = &t.title_key {
            let _ = write!(s, " — title in `{title_key}:`");
        }
        if !t.link_fields.is_empty() {
            let edges: Vec<String> = t
                .link_fields
                .iter()
                .map(|(field, target)| format!("`{field}:` → {target}"))
                .collect();
            let _ = write!(s, "; typed links: {}", edges.join(", "));
        }
        if !t.frontmatter_seed.is_empty() {
            let keys: Vec<&str> = t.frontmatter_seed.keys().map(String::as_str).collect();
            let _ = write!(s, "; seed fields: {}", keys.join(", "));
        }
        s.push('\n');
    }
    let _ = write!(
        s,
        "\nWhen unsure which type to use, default to `{}`.\n\n",
        cfg.default_type
    );

    s.push_str(
        "## Frontmatter\n\n\
Every note begins with a fenced YAML block:\n\n\
```yaml\n\
---\n\
type: <one of the types above>\n\
# put the human title under that type's title key (e.g. topic: or name:)\n\
tags: [optional, tags]\n\
---\n\
```\n\n\
- `type:` must match a node type above.\n\
- Unknown keys are preserved untouched on save — safe to add custom fields.\n\
- A malformed YAML block blocks saving, so keep it valid.\n\n",
    );

    s.push_str(
        "## Linking notes\n\n\
- Use **standard markdown links**: `[Other note](../concepts/other-note.md)`.\n\
- Do **not** use `[[wikilinks]]` — GitNodes does not parse them.\n\
- Typed edges come from the `link_fields` listed above (a frontmatter field whose value \
is the path or slug of another note).\n\
- Shared `tags:` cluster related notes in the graph.\n\n",
    );

    s.push_str(
        "## Adding a note\n\n\
1. Pick the right `type` and create the file in that type's directory.\n\
2. Write valid frontmatter (type + title + any seed fields), then the body in markdown.\n\
3. Link it to related notes with standard markdown links.\n\
4. Commit. The graph rebuilds from the repository.\n",
    );

    s.push_str(
        "\n## Agent tools\n\n\
When the `gitnodes` MCP server is configured in your agent, prefer its read-only \
`search_brain`, `list_nodes`, `read_node`, and `node_links` tools for discovery. They read \
the current working tree through the same projection and search engine as the GitNodes UI. \
Use `node_links` to walk the graph from a note to its incoming and outgoing connections \
instead of guessing relationships from the text.\n",
    );

    let _ = write!(
        s,
        "\n### Connecting the MCP server\n\n\
The command is the same for every client — `gitnodes mcp <path-to-this-repo>`; only where \
the config lives differs. One-line setup for CLI agents:\n\n\
```bash\n\
# Claude Code\n\
claude mcp add gitnodes -- gitnodes mcp \"{repo_path}\"\n\
# Codex CLI\n\
codex mcp add gitnodes -- gitnodes mcp \"{repo_path}\"\n\
```\n\n\
For editors that use a JSON config (Cursor, Antigravity, Cline, Windsurf, Claude Desktop, …), \
add the standard `mcpServers` entry to your client's config file:\n\n\
```json\n\
{{\n  \"mcpServers\": {{\n    \"gitnodes\": {{\n      \"command\": \"gitnodes\",\n      \"args\": [\"mcp\", \"{repo_path}\"]\n    }}\n  }}\n}}\n\
```\n\n\
See your client's MCP documentation for the exact config-file location.\n",
    );

    s
}

/// Write `AGENTS.md` into `root`, generated from `root/.gitnodes.yml` (or the
/// built-in default taxonomy when the repo has no config yet).
fn generate_agents(root: &Path) -> Result<(), String> {
    let cfg_path = root.join(".gitnodes.yml");
    let cfg = if cfg_path.exists() {
        let raw = std::fs::read_to_string(&cfg_path)
            .map_err(|e| format!("failed to read {}: {e}", cfg_path.display()))?;
        gitnodes_domain::BrainConfig::parse(&raw)
            .map_err(|e| format!("{} is invalid: {e}", cfg_path.display()))?
    } else {
        gitnodes_domain::BrainConfig::default()
    };
    let out = root.join("AGENTS.md");
    std::fs::write(&out, render_agents_md(&cfg, root))
        .map_err(|e| format!("failed to write {}: {e}", out.display()))
}

fn preflight_init(root: &Path) -> Result<(), String> {
    if root.exists() && !root.is_dir() {
        return Err(format!("{} exists and is not a directory", root.display()));
    }
    std::fs::create_dir_all(root)
        .map_err(|e| format!("failed to create {}: {e}", root.display()))?;

    let mut collisions = Vec::new();
    for rel in STARTER_FILES
        .iter()
        .map(|(rel, _)| *rel)
        .chain(std::iter::once("AGENTS.md"))
    {
        let path = root.join(rel);
        if path.exists() {
            collisions.push(path);
            continue;
        }

        let mut parent = path.parent();
        while let Some(candidate) = parent {
            if candidate == root {
                break;
            }
            if candidate.exists() && !candidate.is_dir() {
                collisions.push(candidate.to_path_buf());
                break;
            }
            parent = candidate.parent();
        }
    }

    let gitignore = root.join(".gitignore");
    if gitignore.exists() && !gitignore.is_file() {
        collisions.push(gitignore);
    }

    if collisions.is_empty() {
        Ok(())
    } else {
        let paths = collisions
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "refusing to overwrite existing scaffold paths: {paths}"
        ))
    }
}

fn update_gitignore(root: &Path) -> Result<(), String> {
    let path = root.join(".gitignore");
    let existing = if path.exists() {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?
    } else {
        String::new()
    };
    let missing = GITIGNORE_ENTRIES
        .iter()
        .copied()
        .filter(|entry| !existing.lines().any(|line| line.trim() == *entry))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(file).map_err(|e| format!("failed to update {}: {e}", path.display()))?;
    }
    if !existing.is_empty() {
        writeln!(file).map_err(|e| format!("failed to update {}: {e}", path.display()))?;
    }
    writeln!(file, "{GITIGNORE_HEADER}")
        .map_err(|e| format!("failed to update {}: {e}", path.display()))?;
    for entry in missing {
        writeln!(file, "{entry}")
            .map_err(|e| format!("failed to update {}: {e}", path.display()))?;
    }
    Ok(())
}

/// `gitnodes agents [dir]` — (re)generate AGENTS.md from the local config.
pub fn run_agents(dir: Option<&str>) -> Result<(), String> {
    let root = PathBuf::from(dir.unwrap_or("."));
    generate_agents(&root)?;
    println!("Wrote {}", root.join("AGENTS.md").display());
    Ok(())
}

/// Move server startup into the requested knowledge-base checkout before
/// loading `.env` or resolving relative runtime paths.
pub fn enter_serve_dir(dir: Option<&str>) -> Result<(), String> {
    let root = PathBuf::from(dir.unwrap_or("."));
    if !root.is_dir() {
        return Err(format!(
            "serve directory {} does not exist or is not a directory",
            root.display()
        ));
    }
    std::env::set_current_dir(&root).map_err(|e| format!("failed to enter {}: {e}", root.display()))
}

/// Fill the local single-user runtime configuration from the current Git
/// checkout and an existing GitHub CLI login.
///
/// Explicit environment values always win. Discovered credentials are kept in
/// this process only; GitNodes never writes the token to `.env` or another file.
pub fn configure_local_serve() -> Result<Vec<String>, String> {
    let mut notes = Vec::new();

    let target_configured = env_present("TARGET_GITHUB_REPOSITORY")
        || env_present("TARGET_GITHUB_ORG")
        || env_present("GITHUB_ORG")
        || env_present("TARGET_GITHUB_REPO")
        || env_present("GITHUB_REPO");
    let target_discovered = !target_configured;
    if target_discovered {
        let remote =
            command_stdout("git", &["config", "--get", "remote.origin.url"]).map_err(|error| {
                format!(
                    "TARGET_GITHUB_REPOSITORY is unset and the Git remote could not be read: \
                     {error}. Push this checkout to GitHub or set \
                     TARGET_GITHUB_REPOSITORY=owner/repo."
                )
            })?;
        let repository = parse_github_repository(&remote)?;
        // SAFETY: called during single-threaded startup before worker tasks are
        // spawned or application configuration reads the environment.
        unsafe { std::env::set_var("TARGET_GITHUB_REPOSITORY", &repository) };
        notes.push(format!(
            "Using GitHub repository {repository} from remote.origin.url."
        ));
    }

    if target_discovered
        && env_missing("TARGET_GITHUB_BRANCH")
        && env_missing("GITHUB_BRANCH")
        && let Ok(branch) = command_stdout("git", &["branch", "--show-current"])
        && !branch.is_empty()
    {
        // SAFETY: see the startup ordering note above.
        unsafe { std::env::set_var("TARGET_GITHUB_BRANCH", &branch) };
        notes.push(format!("Using current Git branch {branch}."));
    }

    let oauth_id = env_present("GITHUB_CLIENT_ID");
    let oauth_secret = env_present("GITHUB_CLIENT_SECRET");
    if oauth_id != oauth_secret {
        return Err(
            "set both GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET, or unset both to use the \
             local GitHub CLI login"
                .into(),
        );
    }

    if env_missing("GITHUB_PAT") && !oauth_id {
        let token = command_stdout("gh", &["auth", "token"]).map_err(|error| {
            format!(
                "GitNodes needs GitHub access, but no GITHUB_PAT/OAuth credentials were set \
                 and `gh auth token` failed: {error}. Run `gh auth login`, then retry."
            )
        })?;
        if token.is_empty() {
            return Err("`gh auth token` returned an empty credential; run `gh auth login`".into());
        }
        // SAFETY: see the startup ordering note above. The token is inherited
        // from gh's credential store and remains process-local.
        unsafe { std::env::set_var("GITHUB_PAT", token) };
        notes.push("Using your existing GitHub CLI login (token is not persisted).".into());
    }

    Ok(notes)
}

fn env_present(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn env_missing(name: &str) -> bool {
    !env_present(name)
}

fn command_stdout(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run `{program}`: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "`{program} {}` exited with {}",
                args.join(" "),
                output.status
            )
        } else {
            stderr
        });
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| format!("`{program} {}` returned non-UTF-8 output", args.join(" ")))
}

fn parse_github_repository(remote: &str) -> Result<String, String> {
    let remote = remote.trim().trim_end_matches('/');
    let (host, path) = if let Some((_, rest)) = remote.split_once("://") {
        rest.split_once('/')
            .map(|(authority, path)| (authority.rsplit('@').next().unwrap_or(authority), path))
    } else if let Some((authority, path)) = remote.split_once(':') {
        Some((authority.rsplit('@').next().unwrap_or(authority), path))
    } else {
        None
    }
    .ok_or_else(|| {
        format!(
            "cannot infer owner/repo from Git remote {remote:?}; set \
             TARGET_GITHUB_REPOSITORY=owner/repo"
        )
    })?;

    if !host.eq_ignore_ascii_case("github.com") {
        return Err(format!(
            "remote {remote:?} is not hosted on github.com; set \
             TARGET_GITHUB_REPOSITORY=owner/repo explicitly"
        ));
    }
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(format!(
            "cannot infer owner/repo from Git remote {remote:?}; set \
             TARGET_GITHUB_REPOSITORY=owner/repo"
        ));
    }
    Ok(format!("{owner}/{repo}"))
}

/// Scaffold a starter brain into `dir` (default: current directory), `git init`
/// it, and print the next steps to get it served. `Err` carries a message for
/// the caller to print before exiting non-zero.
pub fn run_init(dir: Option<&str>) -> Result<(), String> {
    let root = PathBuf::from(dir.unwrap_or("."));
    preflight_init(&root)?;

    for (rel, contents) in STARTER_FILES {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, contents)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    }

    // Keep runtime secrets and the local projection DB out of the content repo
    // without discarding ignore rules from an existing repository.
    update_gitignore(&root)?;

    // Generate AGENTS.md so coding agents are productive in the brain immediately.
    generate_agents(&root)?;

    // Best-effort `git init` — a brain wants to live in a repo, but a failure
    // here (no git installed) shouldn't fail the scaffold.
    let git_ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let where_ = root.display();
    println!("Scaffolded a starter brain (with AGENTS.md for coding agents) in {where_}.\n");
    println!("Next steps:");
    if !git_ok {
        println!("  0. Install git, then `git init` in {where_}.");
    }
    println!(
        "  1. Explore it locally (no GitHub or login required):\n     \
cd {where_}\n     \
gitnodes preview\n  \
2. Connect an AI agent with the read-only MCP server:\n     \
gitnodes mcp .\n  \
3. Commit the starter knowledge base:\n     \
cd {where_}\n     \
git add . && git commit -m \"Initialize GitNodes knowledge base\"\n  \
4. Create a GitHub repo and push it:\n     \
gh repo create <name> --private --source=. --remote=origin --push\n  \
5. Enable collaborative GitHub-backed editing (reuses your `gh` login):\n     \
gitnodes serve\n"
    );
    Ok(())
}

/// Best-effort: open `url` in the default browser. Silent on failure (headless
/// servers, missing opener) — `serve` always logs the URL regardless.
pub fn open_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else if cfg!(target_os = "windows") {
        // `cmd /C start "" <url>` is the reliable way to hand a URL to the
        // default browser on Windows; the empty "" is start's window-title arg.
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_scaffolds_then_refuses_to_clobber() {
        let dir = std::env::temp_dir().join(format!("gitnodes-init-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let first = run_init(Some(&dir_str));
        // A starter brain landed with config + at least one note + AGENTS.md.
        assert!(first.is_ok());
        assert!(dir.join(".gitnodes.yml").exists());
        assert!(dir.join("concepts/knowledge-graph.md").exists());
        assert!(dir.join("AGENTS.md").exists());

        // Second run must refuse rather than overwrite an existing brain.
        assert!(run_init(Some(&dir_str)).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_preserves_existing_gitignore_and_unrelated_files() {
        let dir = std::env::temp_dir().join(format!(
            "gitnodes-init-existing-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test directory");
        std::fs::write(dir.join("notes.txt"), "keep me").expect("write unrelated file");
        std::fs::write(dir.join(".gitignore"), "target/\n").expect("write existing gitignore");
        let dir_str = dir.to_string_lossy().to_string();

        assert!(run_init(Some(&dir_str)).is_ok());
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).expect("read unrelated file"),
            "keep me"
        );
        let gitignore =
            std::fs::read_to_string(dir.join(".gitignore")).expect("read updated gitignore");
        assert!(gitignore.starts_with("target/\n"));
        assert!(gitignore.lines().any(|line| line == ".env"));
        assert!(gitignore.lines().any(|line| line == "/data/"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_preflights_all_scaffold_collisions_before_writing() {
        let dir = std::env::temp_dir().join(format!(
            "gitnodes-init-collision-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("projects")).expect("create projects directory");
        let collision = dir.join("projects/trial-run.md");
        std::fs::write(&collision, "existing project").expect("write collision");
        let dir_str = dir.to_string_lossy().to_string();

        let error = run_init(Some(&dir_str)).expect_err("collision must abort init");
        assert!(error.contains("refusing to overwrite"));
        assert_eq!(
            std::fs::read_to_string(&collision).expect("read collision"),
            "existing project"
        );
        assert!(!dir.join(".gitnodes.yml").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_md_teaches_conventions() {
        let cfg = gitnodes_domain::BrainConfig::default();
        let md = render_agents_md(&cfg, Path::new("/brains/example"));
        // The non-obvious gotcha every agent must know.
        assert!(md.contains("[[wikilinks]]"));
        assert!(md.contains("standard markdown links"));
        // Node types are enumerated with their directories.
        assert!(md.contains("`concepts/`"));
        assert!(md.contains("GitNodes knowledge base"));
        // The MCP setup commands are filled in with this brain's real path.
        assert!(md.contains("claude mcp add gitnodes -- gitnodes mcp \"/brains/example\""));
        assert!(md.contains("\"args\": [\"mcp\", \"/brains/example\"]"));
    }

    #[test]
    fn parses_common_github_remote_formats() {
        for (remote, expected) in [
            ("https://github.com/acme/notes.git", "acme/notes"),
            ("git@github.com:acme/notes.git", "acme/notes"),
            ("ssh://git@github.com/acme/notes", "acme/notes"),
        ] {
            assert_eq!(parse_github_repository(remote).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_remote_paths_that_are_not_owner_repo() {
        assert!(parse_github_repository("https://example.com/group/acme/notes.git").is_err());
        assert!(parse_github_repository("https://gitlab.com/acme/notes.git").is_err());
        assert!(parse_github_repository("not-a-remote").is_err());
    }
}
