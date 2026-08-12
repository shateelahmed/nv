//! End-to-end smoke tests for `nv changes`, running the compiled binary
//! against throwaway per-service git repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Run git in `dir`, panicking on failure. Inline `-c` flags avoid depending
/// on the developer's git config.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
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

/// Create `root/auth` as its own git repo with baseline branch `base` and a
/// `dev` branch that moves keys between configmap and secrets:
///
/// baseline:  configmap     LOG_LEVEL/DB_PASS/OLD_KEY/CM_MOVED_TO_SECRET
///            secrets       SECRET_MOVED_TO_CM/STAYS_SECRET
///            legacy-secrets LEGACY_KEY            (present on baseline only)
///            dotenv        DOTENV_ONLY            (never scanned by changes)
/// dev:       configmap     LOG_LEVEL/DB_PASS/SECRET_MOVED_TO_CM
///            secrets       CM_MOVED_TO_SECRET/STAYS_SECRET
fn setup_repo(root: &Path, base: &str) -> PathBuf {
    let auth = root.join("auth");
    fs::create_dir_all(&auth).unwrap();
    git(&auth, &["init", "-b", base]);
    fs::write(
        auth.join("configmap.yml"),
        "LOG_LEVEL: info\nDB_PASS: old\nOLD_KEY: gone\nCM_MOVED_TO_SECRET: plainvalue\n",
    )
    .unwrap();
    fs::write(
        auth.join("secrets.yml"),
        "SECRET_MOVED_TO_CM: secretval\nSTAYS_SECRET: keep\n",
    )
    .unwrap();
    fs::write(auth.join("secret-legacy.yml"), "LEGACY_KEY: oldval\n").unwrap();
    fs::write(auth.join(".env"), "DOTENV_ONLY=ignored\n").unwrap();
    git(&auth, &["add", "."]);
    git(&auth, &["commit", "-m", "baseline"]);
    git(&auth, &["checkout", "-b", "dev"]);
    fs::write(
        auth.join("configmap.yml"),
        "LOG_LEVEL: debug\nDB_PASS: new\nSECRET_MOVED_TO_CM: newval\n",
    )
    .unwrap();
    fs::write(
        auth.join("secrets.yml"),
        "CM_MOVED_TO_SECRET: plainvalue\nSTAYS_SECRET: keep\n",
    )
    .unwrap();
    git(&auth, &["rm", "-q", "secret-legacy.yml"]);
    git(&auth, &["add", "."]);
    git(&auth, &["commit", "-m", "dev"]);
    git(&auth, &["checkout", base]);
    auth
}

/// Run the `nv` binary with `cwd` as the working directory.
fn nv(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nv"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn dry_run_lists_moves_redacts_and_groups_by_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "dev",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));

    let text = stdout(&out);
    // Configmap move annotated.
    assert!(text.contains("+ SECRET_MOVED_TO_CM = newval (from secret)"));
    // Updated pair.
    assert!(text.contains("- DB_PASS = old"));
    assert!(text.contains("+ DB_PASS = new"));
    // Deleted bare keys.
    assert!(text.contains("- OLD_KEY"));
    // The move is reported only on the secrets side: the key is no longer
    // listed as Deleted from the configmap.
    assert!(!text.contains("- CM_MOVED_TO_SECRET"));
    // Secrets move with the plain value annotated.
    assert!(text.contains("+ CM_MOVED_TO_SECRET = plainvalue (from configmap)"));
    // Secrets Deleted is bare.
    assert!(text.contains("- SECRET_MOVED_TO_CM"));
    // A file present on the baseline branch only is treated as empty on the
    // from branch: its key shows as Deleted.
    assert!(text.contains("- LEGACY_KEY"));
    // Secrets value that stayed on both branches is never shown.
    assert!(!text.contains("STAYS_SECRET"));
    // The secrets item keeps its own secret value hidden.
    assert!(!text.contains("secretval"));
    // Dotenv files are never scanned by changes.
    assert!(!text.contains("DOTENV_ONLY"));
    assert!(!text.contains(".env"));

    let err = stderr(&out);
    assert!(err.contains("Config source: command-line"));
    assert!(err.contains("7 change(s) found between 'dev' and 'master'."));
}

#[test]
fn default_to_uses_configured_master_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "main");
    fs::write(
        root.join("nv.yml"),
        "services_root: .\ncommands:\n  changes:\n    master_branch: main\n",
    )
    .unwrap();

    let out = nv(
        root,
        &["changes", "--service", "auth", "--from", "dev", "--dry-run"],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));
    let err = stderr(&out);
    // The child resolves the /var symlink (macOS), so canonicalize both sides.
    let expected = std::fs::canonicalize(root.join("nv.yml")).unwrap();
    assert!(err.contains(&format!("Config source: {}", expected.display())));
    assert!(err.contains("7 change(s) found between 'dev' and 'main'."));
}

#[test]
fn default_to_master_without_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");

    let out = nv(
        root,
        &["changes", "--service", "auth", "--from", "dev", "--dry-run"],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));
    assert!(stderr(&out).contains("7 change(s) found between 'dev' and 'master'."));
}

#[test]
fn skip_files_excludes_a_scanned_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");
    fs::write(
        root.join("nv.yml"),
        "services_root: .\ncommands:\n  changes:\n    skip_files:\n      - secrets.yml\n",
    )
    .unwrap();

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "dev",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));
    let text = stdout(&out);
    // secrets.yml is skipped, so the move annotation can no longer apply and
    // the key shows without it.
    assert!(text.contains("+ SECRET_MOVED_TO_CM = newval"));
    assert!(!text.contains("(from secrets)"));
    assert!(!text.contains("CM_MOVED_TO_SECRET = plainvalue"));
    assert!(!text.contains("STAYS_SECRET"));
    // secret-legacy.yml is still scanned, so its deleted key remains.
    assert!(text.contains("- LEGACY_KEY"));
    assert!(stderr(&out).contains("6 change(s) found"));
}

#[test]
fn no_changes_prints_no_changes_found() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "master",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert!(out.status.success());
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("No changes found."));
}

#[test]
fn no_service_errors_before_git_work() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // An empty root: even with no repo anywhere, the error must be the
    // exactly-one-service one, not a git error.
    let out = nv(
        root,
        &["changes", "--from", "dev", "--to", "master", "--dry-run"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("requires exactly one --service"));
}

#[test]
fn more_than_one_service_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");
    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--service",
            "other",
            "--from",
            "dev",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("requires exactly one --service"));
}

#[test]
fn missing_from_flag_is_a_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");
    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--from"));
}

#[test]
fn not_a_git_repository_errors_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let auth = root.join("auth");
    fs::create_dir_all(&auth).unwrap();
    fs::write(auth.join("configmap.yml"), "LOG_LEVEL: info\n").unwrap();

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "dev",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("not inside a git repository"));
}

#[test]
fn missing_branch_errors_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_repo(root, "master");
    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "nope",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("branch 'nope' not found"));
}

/// Create `root/auth` as its own git repo with baseline branch `master` and a
/// `feature` branch, with one configmap/secrets pair per environment folder:
///
/// master:  deploy/dev/configmap  DEV_KEY (devold), SHARED
///          deploy/dev/secrets    DEV_SECRET
///          deploy/prod/configmap PROD_KEY (prodold)
///          deploy/prod/secrets   PROD_SECRET
/// feature: deploy/dev/configmap  DEV_KEY (devnew), SHARED
///          deploy/dev/secrets    DEV_SECRET2           (DEV_SECRET removed)
///          deploy/prod/configmap PROD_KEY (prodnew)
///          deploy/prod/secrets   PROD_SECRET
fn setup_env_repo(root: &Path, base: &str) -> PathBuf {
    let auth = root.join("auth");
    fs::create_dir_all(auth.join("deploy/dev")).unwrap();
    fs::create_dir_all(auth.join("deploy/prod")).unwrap();
    git(&auth, &["init", "-b", base]);
    fs::write(
        auth.join("deploy/dev/configmap.yml"),
        "DEV_KEY: devold\nSHARED: one\n",
    )
    .unwrap();
    fs::write(auth.join("deploy/dev/secrets.yml"), "DEV_SECRET: ds\n").unwrap();
    fs::write(
        auth.join("deploy/prod/configmap.yml"),
        "PROD_KEY: prodold\n",
    )
    .unwrap();
    fs::write(auth.join("deploy/prod/secrets.yml"), "PROD_SECRET: ps\n").unwrap();
    git(&auth, &["add", "."]);
    git(&auth, &["commit", "-m", "baseline"]);
    git(&auth, &["checkout", "-b", "feature"]);
    fs::write(
        auth.join("deploy/dev/configmap.yml"),
        "DEV_KEY: devnew\nSHARED: one\n",
    )
    .unwrap();
    fs::write(auth.join("deploy/dev/secrets.yml"), "DEV_SECRET2: ds2\n").unwrap();
    fs::write(
        auth.join("deploy/prod/configmap.yml"),
        "PROD_KEY: prodnew\n",
    )
    .unwrap();
    git(&auth, &["add", "."]);
    git(&auth, &["commit", "-m", "feature"]);
    git(&auth, &["checkout", base]);
    auth
}

#[test]
fn environment_flag_filters_to_one_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_env_repo(root, "master");

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "feature",
            "--to",
            "master",
            "--environment",
            "dev",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("+ DEV_KEY = devnew"), "dev updated:\n{text}");
    assert!(
        text.contains("- DEV_KEY = devold"),
        "dev updated pair:\n{text}"
    );
    assert!(
        text.contains("+ DEV_SECRET2"),
        "dev secret created:\n{text}"
    );
    assert!(text.contains("- DEV_SECRET"), "dev secret deleted:\n{text}");
    // Prod files are not scanned.
    assert!(!text.contains("PROD_KEY"), "prod filtered out:\n{text}");
    assert!(!text.contains("PROD_SECRET"), "prod filtered out:\n{text}");
    assert!(stderr(&out).contains("3 change(s) found"));
}

#[test]
fn environment_config_filters_to_one_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_env_repo(root, "master");
    fs::write(
        root.join("nv.yml"),
        "services_root: .\ncommands:\n  changes:\n    environment: dev\n",
    )
    .unwrap();

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "feature",
            "--to",
            "master",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "failed:\n{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("+ DEV_KEY = devnew"));
    assert!(!text.contains("PROD_KEY"), "prod filtered out:\n{text}");
    // The child resolves the /var symlink (macOS), so canonicalize both sides.
    let expected = std::fs::canonicalize(root.join("nv.yml")).unwrap();
    assert!(stderr(&out).contains(&format!("Config source: {}", expected.display())));
}

#[test]
fn unknown_environment_errors_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    setup_env_repo(root, "master");

    let out = nv(
        root,
        &[
            "changes",
            "--service",
            "auth",
            "--from",
            "feature",
            "--to",
            "master",
            "--environment",
            "staging",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("environment 'staging' has no configmap/secrets files"),
        "{}",
        stderr(&out)
    );
}
