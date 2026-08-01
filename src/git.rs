//! Reading repository, branch, and file information through the `git` CLI.
//!
//! `nv changes` compares a service's env files between two branches. Instead of
//! mutating the user's checkout, this module reads content straight from the
//! object database via `git show <branch>:<path>`. Every command runs in the
//! repository's own directory (`git -C <repo>`) and passes arguments directly
//! (never through a shell), so unusual branch or file names cannot inject
//! commands.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Run `git -C <repo> <args>` and return its output. Errors only on the
/// process itself failing (e.g. git not installed); a non-zero exit status is
/// the caller's concern.
fn run_git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("running git in {}", repo.display()))
}

/// Resolve the repository root containing `dir` (the directory itself or a
/// parent). Fails when `dir` is not inside a git repository.
pub fn repo_root(dir: &Path) -> Result<PathBuf> {
    let out = run_git(dir, &["rev-parse", "--show-toplevel"])?;
    if !out.status.success() {
        bail!("not inside a git repository");
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Whether `branch` exists as a local branch in `repo`.
pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let spec = format!("refs/heads/{branch}");
    let out = run_git(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
    Ok(out.status.success())
}

/// Read a file's content at `branch`, or `None` when the file does not exist
/// on that branch. Non-UTF-8 content is decoded lossily (config/env files are
/// text in practice).
pub fn read_file_at(repo: &Path, branch: &str, rel: &str) -> Result<Option<String>> {
    let spec = format!("{branch}:{rel}");
    let exists = run_git(repo, &["cat-file", "-e", &spec])?;
    if !exists.status.success() {
        return Ok(None);
    }
    let out = run_git(repo, &["show", &spec])?;
    if !out.status.success() {
        bail!("git show {spec} failed");
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The forward-slash path of `abs` relative to the repository root `repo`
/// (what `git show <branch>:<path>` expects). Fails when `abs` is outside the
/// repository.
pub fn rel_to_repo(repo: &Path, abs: &Path) -> Result<String> {
    let rel = abs
        .strip_prefix(repo)
        .context("file is not inside the git repository")?;
    let mut parts: Vec<String> = Vec::new();
    for component in rel.components() {
        if let Component::Normal(part) = component {
            parts.push(part.to_string_lossy().into_owned());
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Run git in `dir`, panicking on failure, so tests have a real repo to
    /// probe. Inline `-c` flags avoid depending on the developer's git config.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap()])
            .args(["-c", "user.email=test@example.com"])
            .args(["-c", "user.name=test"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Create a throwaway repository with a `master` branch and a `dev` branch
    /// holding different versions of `configmap.yml`.
    fn make_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("svc");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "master"]);
        fs::write(
            repo.join("configmap.yml"),
            "DB_PASS: old\nLOG_LEVEL: debug\n",
        )
        .unwrap();
        git(&repo, &["add", "configmap.yml"]);
        git(&repo, &["commit", "-m", "master"]);
        git(&repo, &["checkout", "-b", "dev"]);
        fs::write(repo.join("configmap.yml"), "DB_PASS: new\nAPI_KEY: abc\n").unwrap();
        git(&repo, &["add", "configmap.yml"]);
        git(&repo, &["commit", "-m", "dev"]);
        git(&repo, &["checkout", "master"]);
        (tmp, repo)
    }

    #[test]
    fn repo_root_is_repo_dir_for_service_roots() {
        let (_tmp, repo) = make_repo();
        // git resolves symlinks (e.g. /var → /private/var on macOS), so compare
        // against the canonicalized directory.
        let expected = repo.canonicalize().unwrap();
        assert_eq!(repo_root(&repo).unwrap(), expected);
    }

    #[test]
    fn repo_root_finds_parent_repo_from_subdir() {
        let (_tmp, repo) = make_repo();
        let sub = repo.join("nested");
        fs::create_dir_all(&sub).unwrap();
        let expected = repo.canonicalize().unwrap();
        assert_eq!(repo_root(&sub).unwrap(), expected);
    }

    #[test]
    fn repo_root_errors_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(repo_root(tmp.path()).is_err());
    }

    #[test]
    fn branch_exists_for_local_branches() {
        let (_tmp, repo) = make_repo();
        assert!(branch_exists(&repo, "master").unwrap());
        assert!(branch_exists(&repo, "dev").unwrap());
        assert!(!branch_exists(&repo, "nope").unwrap());
    }

    #[test]
    fn read_file_at_reads_each_branch_version() {
        let (_tmp, repo) = make_repo();
        let master = read_file_at(&repo, "master", "configmap.yml")
            .unwrap()
            .expect("file exists on master");
        assert_eq!(master, "DB_PASS: old\nLOG_LEVEL: debug\n");
        let dev = read_file_at(&repo, "dev", "configmap.yml")
            .unwrap()
            .expect("file exists on dev");
        assert_eq!(dev, "DB_PASS: new\nAPI_KEY: abc\n");
    }

    #[test]
    fn read_file_at_returns_none_for_missing_file() {
        let (_tmp, repo) = make_repo();
        assert_eq!(read_file_at(&repo, "master", "secrets.yml").unwrap(), None);
    }

    #[test]
    fn rel_to_repo_strips_repo_root() {
        let (_tmp, repo) = make_repo();
        let abs = repo.join("deploy").join("secrets.yml");
        assert_eq!(rel_to_repo(&repo, &abs).unwrap(), "deploy/secrets.yml");
    }
}
