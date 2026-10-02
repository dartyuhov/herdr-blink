//! Repo / branch discovery by reading `.git` files directly (no `git`
//! subprocesses). Results are cached per cwd for the popup's lifetime.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitInfo {
    /// Worktree root basename (the "project").
    pub project: String,
    /// Repository name: origin remote basename, else the main checkout's dir.
    pub repo: String,
    /// Branch name, or a short sha when HEAD is detached.
    pub branch: String,
    /// Linked worktree name, when the cwd is inside one.
    pub worktree: Option<String>,
}

#[derive(Default)]
pub struct GitCache {
    by_path: HashMap<PathBuf, Option<GitInfo>>,
}

impl GitCache {
    pub fn lookup(&mut self, cwd: &Path) -> Option<GitInfo> {
        if let Some(hit) = self.by_path.get(cwd) {
            return hit.clone();
        }
        let info = discover(cwd);
        self.by_path.insert(cwd.to_path_buf(), info.clone());
        info
    }
}

pub fn discover(cwd: &Path) -> Option<GitInfo> {
    let mut dir = Some(cwd);
    while let Some(current) = dir {
        let dot_git = current.join(".git");
        if let Ok(meta) = fs::metadata(&dot_git) {
            return if meta.is_dir() {
                Some(info_for(current, &dot_git, &dot_git, None))
            } else {
                linked_info(current, &dot_git)
            };
        }
        dir = current.parent();
    }
    None
}

/// `.git` is a file: a linked worktree or a submodule checkout.
fn linked_info(root: &Path, dot_git_file: &Path) -> Option<GitInfo> {
    let content = fs::read_to_string(dot_git_file).ok()?;
    let gitdir = content
        .lines()
        .find_map(|l| l.strip_prefix("gitdir:"))?
        .trim();
    let gitdir = root.join(gitdir);
    let common = fs::read_to_string(gitdir.join("commondir"))
        .ok()
        .map(|c| gitdir.join(c.trim()))
        .unwrap_or_else(|| gitdir.clone());
    let worktree = gitdir
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "worktrees"))
        .and_then(|_| gitdir.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    Some(info_for(root, &gitdir, &common, worktree))
}

fn info_for(root: &Path, gitdir: &Path, common: &Path, worktree: Option<String>) -> GitInfo {
    let project = basename(root);
    let repo = fs::read_to_string(common.join("config"))
        .ok()
        .and_then(|c| origin_repo_name(&c))
        .or_else(|| {
            // `<main checkout>/.git` → main checkout dir name.
            let common = fs::canonicalize(common).unwrap_or_else(|_| common.to_path_buf());
            if common.file_name()? != ".git" {
                return None;
            }
            Some(basename(common.parent()?))
        })
        .unwrap_or_else(|| project.clone());
    let branch = fs::read_to_string(gitdir.join("HEAD"))
        .ok()
        .map(|head| parse_head(&head))
        .unwrap_or_default();
    GitInfo {
        project,
        repo,
        branch,
        worktree,
    }
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn parse_head(head: &str) -> String {
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(r) => {
            let r = r.trim();
            r.strip_prefix("refs/heads/").unwrap_or(r).to_string()
        }
        None => head.chars().take(7).collect(),
    }
}

/// Finds `[remote "origin"] url = …` and returns the repo name from it.
pub fn origin_repo_name(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "url" {
            continue;
        }
        let url = value.trim().trim_end_matches('/');
        let name = url.rsplit(['/', ':']).next()?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        return (!name.is_empty()).then(|| name.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_head_refs_and_detached() {
        assert_eq!(parse_head("ref: refs/heads/feat/x\n"), "feat/x");
        assert_eq!(parse_head("0123456789abcdef\n"), "0123456");
    }

    #[test]
    fn parses_origin_url() {
        let cfg = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@github.com:a/other.git\n[remote \"origin\"]\n\turl = git@github.com:dartyuhov/herdr-blink.git\n";
        assert_eq!(origin_repo_name(cfg).as_deref(), Some("herdr-blink"));
        assert_eq!(
            origin_repo_name("[remote \"origin\"]\nurl = https://x.dev/a/b/\n").as_deref(),
            Some("b")
        );
        assert_eq!(origin_repo_name("[core]\n"), None);
    }

    #[test]
    fn discovers_plain_repo_and_linked_worktree() {
        let tmp = std::env::temp_dir().join(format!("blink-git-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let main = tmp.join("myrepo");
        fs::create_dir_all(main.join(".git/worktrees/wt1")).unwrap();
        fs::create_dir_all(main.join("src/deep")).unwrap();
        fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(main.join(".git/config"), "[core]\n").unwrap();
        fs::write(
            main.join(".git/worktrees/wt1/HEAD"),
            "ref: refs/heads/topic\n",
        )
        .unwrap();
        fs::write(main.join(".git/worktrees/wt1/commondir"), "../..\n").unwrap();
        let wt = tmp.join("myrepo-wt1");
        fs::create_dir_all(&wt).unwrap();
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/wt1").display()),
        )
        .unwrap();

        let info = discover(&main.join("src/deep")).unwrap();
        assert_eq!(
            info,
            GitInfo {
                project: "myrepo".into(),
                repo: "myrepo".into(),
                branch: "main".into(),
                worktree: None
            }
        );

        let info = discover(&wt).unwrap();
        assert_eq!(info.project, "myrepo-wt1");
        assert_eq!(info.repo, "myrepo");
        assert_eq!(info.branch, "topic");
        assert_eq!(info.worktree.as_deref(), Some("wt1"));

        fs::remove_dir_all(&tmp).unwrap();
    }
}
