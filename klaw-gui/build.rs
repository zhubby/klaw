use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let Some(workspace_root) = manifest_dir().and_then(|dir| dir.parent().map(Path::to_path_buf))
    else {
        return;
    };

    emit_git_rerun_hints(&workspace_root);

    if let Some(commit_sha) = git_commit_sha(&workspace_root) {
        println!("cargo:rustc-env=KLAW_GIT_COMMIT_SHA={commit_sha}");
    }
}

fn manifest_dir() -> Option<PathBuf> {
    env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from)
}

fn emit_git_rerun_hints(workspace_root: &Path) {
    let git_dir = workspace_root.join(".git");
    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());

    let Ok(head_contents) = fs::read_to_string(&head_path) else {
        return;
    };

    let Some(reference) = head_contents.strip_prefix("ref: ").map(str::trim) else {
        return;
    };

    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join(reference).display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );
}

fn git_commit_sha(workspace_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sha = String::from_utf8(output.stdout).ok()?;
    let sha = sha.trim();
    (!sha.is_empty()).then_some(sha.to_owned())
}
