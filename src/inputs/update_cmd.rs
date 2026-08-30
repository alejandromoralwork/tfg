//! The `update` command: fetches the latest source of this project's own
//! repository (`github.com/alejandromoralwork/tfg`), builds it in a fresh
//! scratch directory, and relaunches the freshly-built binary in place of
//! this process.
//!
//! Deliberately avoids requiring `git` to be installed: GitHub serves any
//! branch as a plain `.tar.gz` snapshot straight from `codeload.github.com`
//! — no auth, no git protocol, just an HTTPS GET. `curl` + `tar` are the
//! same two external tools `inputs::download_cmd` already relies on for
//! exactly this reason (see that module's own doc comment) — this adds no
//! new dependency, tool, or protocol to the project. Unlike that module's
//! `.tar.xz` archives, a `.tar.gz` needs no two-step decompress-then-extract
//! workaround: gzip is bsdtar's (Windows' bundled `tar.exe`) own built-in
//! codec, only LZMA needed the external-pipe dance.
//!
//! Never touches this process's own checkout or working directory: the
//! fresh source is downloaded, extracted, and built entirely under a new
//! directory in the OS temp folder, and only the final *relaunch* step
//! depends on that build having succeeded. This sidesteps the classic
//! self-update problem of not being able to overwrite a binary file while
//! it's the one currently running (a hard Windows restriction — a locked
//! executable can't be rewritten by another process while it's running) —
//! there's simply nothing of the running process's own to overwrite.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use colored::Colorize;

const REPO_OWNER: &str = "alejandromoralwork";
const REPO_NAME: &str = "tfg";
const BIN_NAME: &str = "market_sim";

/// `branch` defaults to `main` when not given.
pub fn run(branch: Option<&str>) {
    let branch = branch.unwrap_or("main");
    let work_dir = std::env::temp_dir().join(format!("{REPO_NAME}-update-{}-{}", std::process::id(), unix_secs()));

    println!("{}", format!("==> Updating from https://github.com/{REPO_OWNER}/{REPO_NAME} ({branch}), no git required ...").cyan());
    println!("    Scratch directory: {}", work_dir.display());
    std::io::stdout().flush().ok();

    if let Err(err) = fs::create_dir_all(&work_dir) {
        println!("{}", format!("[ERROR] Couldn't create '{}': {err}", work_dir.display()).red());
        return;
    }

    let Some(extracted_root) = download_and_extract(&work_dir, branch) else {
        // Nothing salvageable to leave around on a download/extract
        // failure — unlike a build failure, there's no source worth
        // inspecting.
        fs::remove_dir_all(&work_dir).ok();
        return;
    };

    let Some(new_exe) = build(&extracted_root) else {
        println!("{}", format!("        Source left at '{}' for inspection.", extracted_root.display()).yellow());
        return;
    };

    println!("{}", format!("[OK] Build succeeded: {}", new_exe.display()).green());
    println!("{}", "==> Relaunching the updated build — this session will end now ...".cyan());
    std::io::stdout().flush().ok();

    relaunch(&new_exe);
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Downloads `https://codeload.github.com/{owner}/{repo}/tar.gz/refs/heads/{branch}`
/// into `work_dir` and extracts it, returning the path to the single
/// top-level directory the tarball unpacked into (GitHub names it
/// `{repo}-{branch}`, but this doesn't hardcode that — it just looks for
/// whatever one directory extraction actually produced, so a naming
/// convention change upstream can't silently break this).
fn download_and_extract(work_dir: &Path, branch: &str) -> Option<PathBuf> {
    let url = format!("https://codeload.github.com/{REPO_OWNER}/{REPO_NAME}/tar.gz/refs/heads/{branch}");
    let tarball = work_dir.join("source.tar.gz");

    println!("    Downloading {url}");
    std::io::stdout().flush().ok();

    // -f: nonzero exit on an HTTP error (e.g. an unknown branch/tag)
    // instead of writing the error page to the tarball. -S: still show
    // curl's own error even though nothing else is silenced. -L: follow
    // redirects, matching download_cmd's own curl invocation.
    let curl_status = Command::new("curl").args(["-fSL", "-o"]).arg(&tarball).arg(&url).status();
    match curl_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] curl exited with {status} — download failed. Check the branch name ('{branch}') and your network connection.").red());
            return None;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'curl' ({err}). Is curl on PATH?").red());
            return None;
        }
    }

    println!("    Extracting ...");
    std::io::stdout().flush().ok();

    let tar_status = Command::new("tar").arg("-xzf").arg(&tarball).arg("-C").arg(work_dir).status();
    match tar_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] tar exited with {status} — extraction failed.").red());
            return None;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'tar' ({err}). Is tar on PATH?").red());
            return None;
        }
    }

    fs::read_dir(work_dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir())
}

/// `cargo build --release` against the freshly extracted source, streaming
/// the compiler's own output live (`.status()`, inherited stdio) rather
/// than capturing it — a full rebuild of this crate is easily long enough
/// that silent, buffered progress would look hung.
fn build(extracted_root: &Path) -> Option<PathBuf> {
    // This project's own layout: Cargo.toml lives in `src/`, not the repo
    // root (see the top-level Cargo.toml's own comment on why) — same
    // convention the just-downloaded source will have, since it's this
    // exact repository.
    let src_dir = extracted_root.join("src");
    if !src_dir.join("Cargo.toml").is_file() {
        println!("{}", format!("[ERROR] Downloaded source doesn't have the expected 'src/Cargo.toml' layout under '{}'.", extracted_root.display()).red());
        return None;
    }

    println!("    Building (cargo build --release) — this can take a while ...");
    std::io::stdout().flush().ok();

    let build_status = Command::new("cargo").args(["build", "--release"]).current_dir(&src_dir).status();
    match build_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] Build failed ({status}) — see the compiler output above.").red());
            return None;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'cargo' ({err}). Is cargo on PATH?").red());
            return None;
        }
    }

    let exe_name = if cfg!(windows) { format!("{BIN_NAME}.exe") } else { BIN_NAME.to_string() };
    let exe_path = src_dir.join("target").join("release").join(&exe_name);
    if exe_path.is_file() {
        Some(exe_path)
    } else {
        println!("{}", format!("[ERROR] Build reported success but the expected binary wasn't found at '{}'.", exe_path.display()).red());
        None
    }
}

/// Hands off to `new_exe` and ends this process. On Unix this literally
/// replaces the current process image (`exec` — no parent left behind at
/// all); on Windows (no such syscall) it spawns `new_exe` as a normal
/// child inheriting this console, then exits — the practical equivalent,
/// just with a parent that briefly outlives the handoff instead of none.
fn relaunch(new_exe: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only returns on failure — success replaces this process entirely.
        let err = Command::new(new_exe).exec();
        println!("{}", format!("[ERROR] Failed to relaunch ({err}). The updated build is at '{}' — run it manually.", new_exe.display()).red());
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        match Command::new(new_exe).spawn() {
            Ok(_) => std::process::exit(0),
            Err(err) => {
                println!("{}", format!("[ERROR] Failed to relaunch ({err}). The updated build is at '{}' — run it manually.", new_exe.display()).red());
                std::process::exit(1);
            }
        }
    }
}
