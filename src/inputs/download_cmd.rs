//! The `download` and `extract` commands: fetches BTC/ETH/SOL order-status
//! archives from the same Zenodo record `data/download_data.sh` already
//! uses (<https://zenodo.org/records/18184441>), skipping the `_rejected`
//! variants (the engines never need them — see `Order::is_new_live_order`),
//! and extracts them ready for `simulate <btc|eth|sol>` to consume.
//! `extract` is the second half of that (untar only, no curl) run on its
//! own — for retrying just the extraction step against an archive that's
//! already local, e.g. after `download`'s own extraction step failed.
//!
//! Shells out to `curl` and `tar` rather than adding an HTTP client + LZMA
//! decoder as real Cargo dependencies — both ship natively on this machine
//! (`C:\Windows\system32\curl.exe`/`tar.exe`, Windows 10 1803+), and
//! `download_data.sh` already relies on exactly the same two tools for
//! exactly this dataset. Consistent with why this project added `flate2`
//! only after concluding hand-rolling gzip wasn't reasonable — the same
//! logic argues against hand-rolling curl+tar here.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use colored::Colorize;

use crate::inputs::progress;

const ZENODO_BASE: &str = "https://zenodo.org/records/18184441/files";
const ARCHIVE_SUFFIX: &str = "orders_202512.tar.xz";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coin {
    Btc,
    Eth,
    Sol,
}

impl Coin {
    pub fn label(self) -> &'static str {
        match self {
            Coin::Btc => "btc",
            Coin::Eth => "eth",
            Coin::Sol => "sol",
        }
    }

    pub fn all() -> [Coin; 3] {
        [Coin::Btc, Coin::Eth, Coin::Sol]
    }

    fn archive_filename(self) -> String {
        format!("{}_{ARCHIVE_SUFFIX}", self.label())
    }

    fn url(self) -> String {
        format!("{ZENODO_BASE}/{}?download=1", self.archive_filename())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadTarget {
    Coin(Coin),
    All,
}

pub fn run(target: DownloadTarget) {
    match target {
        DownloadTarget::Coin(coin) => download_one(coin),
        DownloadTarget::All => {
            for coin in Coin::all() {
                download_one(coin);
            }
        }
    }
}

/// Same shape as `run`, but skips `curl` entirely — only (re-)extracts an
/// archive that's already sitting on disk, e.g. left behind by a `download`
/// whose extraction step failed (that failure message points here). Useful
/// after fixing whatever made extraction fail (tar/xz on PATH, disk space,
/// ...) without re-fetching a multi-GB file that's already local.
pub fn run_extract(target: DownloadTarget) {
    match target {
        DownloadTarget::Coin(coin) => extract_one(coin),
        DownloadTarget::All => {
            for coin in Coin::all() {
                extract_one(coin);
            }
        }
    }
}

fn download_one(coin: Coin) {
    let label = coin.label();
    let data_root = Path::new("data");
    let dest = data_root.join("order_statuses").join(label);

    if let Err(err) = fs::create_dir_all(&dest) {
        println!("{}", format!("[ERROR] Failed to create '{}': {err}", dest.display()).red());
        return;
    }

    if dest_already_populated(&dest) {
        println!("{}", format!("[OK] '{}' already has data — skipping download (delete it first to re-fetch).", dest.display()).green());
        return;
    }

    let archive = data_root.join(coin.archive_filename());
    let url = coin.url();

    println!("{}", format!("==> Downloading {label} order-status data from Zenodo ...").cyan());
    println!("   {url}");
    std::io::stdout().flush().ok();

    // -L: follow redirects. -C -: resume a partial download (or skip the
    // transfer entirely if the file is already complete). --fail: nonzero
    // exit on an HTTP error instead of writing the error page to the
    // archive. --retry 3: same resilience download_data.sh already relies
    // on for these same multi-GB files. --retry-all-errors: without this,
    // --retry only covers curl's default "transient" error class (timeouts,
    // HTTP 5xx, etc.) — a mid-transfer TLS drop (e.g. Windows Schannel's
    // "server closed abruptly (missing close_notify)", seen on these
    // multi-GB Zenodo downloads) isn't in that set and would otherwise fail
    // the whole command on one bad connection instead of retrying (resuming
    // via -C -, not restarting).
    let curl_status = Command::new("curl").args(["-L", "-C", "-", "--fail", "--retry", "3", "--retry-all-errors", "-o"]).arg(&archive).arg(&url).status();

    match curl_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] curl exited with {status} — download failed or the archive doesn't exist at that URL.").red());
            println!("{}", format!("        If this keeps happening, download it manually and save it as '{}':", archive.display()).yellow());
            println!("        {url}");
            return;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'curl' ({err}). Is curl.exe on PATH?").red());
            println!("{}", format!("        You can also download it manually and save it as '{}':", archive.display()).yellow());
            println!("        {url}");
            return;
        }
    }

    extract_archive(label, &archive, &dest, &url);
}

/// The `extract <coin>` command: extracts an archive already sitting on
/// disk (from a prior `download` whose curl step succeeded), without
/// touching the network at all.
fn extract_one(coin: Coin) {
    let label = coin.label();
    let data_root = Path::new("data");
    let dest = data_root.join("order_statuses").join(label);

    if let Err(err) = fs::create_dir_all(&dest) {
        println!("{}", format!("[ERROR] Failed to create '{}': {err}", dest.display()).red());
        return;
    }

    if dest_already_populated(&dest) {
        println!("{}", format!("[OK] '{}' already has data — nothing to extract.", dest.display()).green());
        return;
    }

    let archive = data_root.join(coin.archive_filename());
    let url = coin.url();

    if !archive.is_file() {
        println!("{}", format!("[ERROR] No downloaded archive found at '{}'.", archive.display()).red());
        println!("{}", "        Run 'download <coin>' first, or download it manually from:".yellow());
        println!("        {url}");
        return;
    }

    extract_archive(label, &archive, &dest, &url);
}

/// Extracts an already-fully-downloaded `archive` into `dest`, shared by
/// both `download_one` (right after curl succeeds) and `extract_one` (the
/// standalone `extract` command). Reports its own errors and cleans up the
/// archive on success — callers have nothing left to do afterward either
/// way.
fn extract_archive(label: &str, archive: &Path, dest: &Path, url: &str) {
    // xz's own index (at the end of the file) records the *uncompressed*
    // size without decompressing anything — near-instant even on this
    // multi-GB archive (~0.1s, measured). Gives both progress bars below a
    // real percentage instead of just an unbounded byte count.
    // `None` (xz not found, or an unexpected `--robot` format from some xz
    // version) just means the bars degrade to showing bytes done with no
    // percentage — never fatal to the extraction itself.
    let total_bytes = xz_uncompressed_size(archive);

    // Two separate steps rather than one `tar -xf archive.tar.xz` — Windows'
    // bundled tar.exe (bsdtar) has no LZMA support of its own, so it shells
    // out to an external `xz` and pipes the whole multi-GB stream through
    // it internally. In practice that combination deadlocks on large
    // archives on this platform (confirmed by hand: tar+xz both sit at
    // ~4KB memory and zero output for as long as you let them run — not
    // slow, genuinely stuck). Decompressing to a plain `.tar` file first,
    // then extracting *that* with no filter involved, sidesteps the
    // deadlock-prone pipe entirely — plain file I/O on one side, tar's
    // simplest/most-exercised code path on the other.
    let tar_path = archive.with_extension(""); // "sol_orders_202512.tar.xz" -> "...tar"

    println!("{}", format!("==> Decompressing {} ...", archive.display()).cyan());
    std::io::stdout().flush().ok();

    let mut xz_cmd = Command::new("xz");
    xz_cmd.arg("-dk").arg(archive); // -d decompress, -k keep the .xz (we remove it ourselves on full success)
    if let Some(path_for_xz) = path_with_bundled_xz() {
        xz_cmd.env("PATH", path_for_xz);
    }
    let mut xz_child = match xz_cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'xz' ({err}). Is xz.exe on PATH? Archive left at '{}'.", archive.display()).red());
            println!("{}", "        If it keeps failing, decompress+extract that archive manually (e.g. with 7-Zip), or re-download it from:".yellow());
            println!("        {url}");
            return;
        }
    };
    let decompress_status = progress::run_with_progress(total_bytes, || fs::metadata(&tar_path).map(|m| m.len()).unwrap_or(0), progress::human_bytes, || String::new(), || xz_child.wait());
    match decompress_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] xz exited with {status} — decompression failed. Archive left at '{}' for a manual retry.", archive.display()).red());
            println!("{}", "        If it keeps failing, decompress+extract that archive manually (e.g. with 7-Zip), or re-download it from:".yellow());
            println!("        {url}");
            return;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] xz failed unexpectedly while decompressing ({err}). Archive left at '{}' for a manual retry.", archive.display()).red());
            println!("{}", "        If it keeps failing, decompress+extract that archive manually (e.g. with 7-Zip), or re-download it from:".yellow());
            println!("        {url}");
            return;
        }
    }

    println!("{}", format!("==> Extracting {} -> {}/ ...", tar_path.display(), dest.display()).cyan());
    std::io::stdout().flush().ok();

    let mut tar_cmd = Command::new("tar");
    tar_cmd.arg("-xf").arg(&tar_path).arg("-C").arg(dest); // plain .tar now — no filter, no external process for tar to shell out to
    let mut tar_child = match tar_cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't run 'tar' ({err}). Is tar.exe on PATH? Decompressed archive left at '{}' for a manual retry.", tar_path.display()).red());
            return;
        }
    };
    let tar_status = progress::run_with_progress(total_bytes, || dir_size(dest), progress::human_bytes, || String::new(), || tar_child.wait());
    let final_bytes = dir_size(dest);
    match tar_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("{}", format!("[ERROR] tar exited with {status} — extraction failed. Decompressed archive left at '{}' for a manual retry.", tar_path.display()).red());
            return;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] tar failed unexpectedly while extracting ({err}). Decompressed archive left at '{}' for a manual retry.", tar_path.display()).red());
            return;
        }
    }

    // Multi-GB files — not worth keeping either of once extracted, matching
    // download_data.sh's default behavior for the original archive.
    if let Err(err) = fs::remove_file(&tar_path) {
        println!("{}", format!("[WARN] Extracted OK, but couldn't remove decompressed '{}': {err}", tar_path.display()).yellow());
    }
    if let Err(err) = fs::remove_file(archive) {
        println!("{}", format!("[WARN] Extracted OK, but couldn't remove archive '{}': {err}", archive.display()).yellow());
    }

    println!("{}", format!("[OK] {label} order-status data ready at '{}' ({}). Try: simulate {label}", dest.display(), progress::human_bytes(final_bytes)).green());
}

/// Total size in bytes of every regular file under `dir` (recursive) — the
/// progress bar's "bytes extracted so far" signal, read directly off disk
/// rather than trusting any counter of our own.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

/// The *uncompressed* size of a `.tar.xz` archive, read from xz's own
/// end-of-file index (`xz -l --robot`) — a near-instant lookup (no
/// decompression) even on a multi-GB file, since that index is exactly
/// what makes `xz -l` fast in the first place. `None` if `xz` can't be run
/// at all or its machine-readable output doesn't parse as expected;
/// callers treat that as "no total available", not an error.
fn xz_uncompressed_size(archive: &Path) -> Option<u64> {
    let mut cmd = Command::new("xz");
    if let Some(path) = path_with_bundled_xz() {
        cmd.env("PATH", path);
    }
    let output = cmd.args(["-l", "--robot"]).arg(archive).output().ok()?;
    if !output.status.success() {
        return None;
    }
    // --robot's stable, tab-separated format: a line per archive starting
    // with "file", whose 5th field (index 4) is the uncompressed byte
    // count — see `xz(1)`'s ROBOT MODE section.
    String::from_utf8_lossy(&output.stdout).lines().find_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        (fields.first() == Some(&"file")).then(|| fields.get(4)?.parse().ok()).flatten()
    })
}

/// Whether `xz` is already runnable via this process's own PATH — a quiet
/// probe (both streams discarded), not a print-something check.
fn xz_on_path() -> bool {
    Command::new("xz").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// The handful of places a bundled `xz.exe` realistically lives on a
/// Windows dev machine that doesn't have standalone xz-utils installed —
/// Git for Windows ships one (in both its MSYS `usr\bin` and MinGW
/// `mingw64\bin` trees) under whichever `Program Files` variant it was
/// installed into.
fn xz_search_dirs() -> impl Iterator<Item = PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].into_iter().filter_map(std::env::var_os).flat_map(|program_files| {
        let git = PathBuf::from(program_files).join("Git");
        [git.join("mingw64").join("bin"), git.join("usr").join("bin")].into_iter()
    })
}

/// If `xz` isn't already reachable, but a bundled copy turns up in one of
/// `xz_search_dirs`, returns the `PATH` value to hand a *child process*
/// (`tar`, or a direct `xz` invocation) so it can find it — without
/// modifying this process's own (or the user's) actual `PATH`. `None` means
/// "leave PATH alone": either `xz` is already reachable, or no bundled copy
/// was found (in which case the child fails exactly as it would have
/// before, with its usual error message).
fn path_with_bundled_xz() -> Option<OsString> {
    if xz_on_path() {
        return None;
    }
    let found = xz_search_dirs().find(|dir| dir.join("xz.exe").is_file())?;
    let mut path_for_child = found.into_os_string();
    path_for_child.push(";");
    path_for_child.push(std::env::var_os("PATH").unwrap_or_default());
    Some(path_for_child)
}

/// True if `dest` already contains at least one `.gz`/`.data` file — used
/// to skip a redundant multi-GB re-download rather than silently starting
/// over every time `download` is run again.
fn dest_already_populated(dest: &Path) -> bool {
    fn has_data_file(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else { return false };
        for entry in entries.flatten() {
            let path: PathBuf = entry.path();
            if path.is_dir() {
                if has_data_file(&path) {
                    return true;
                }
            } else if matches!(path.extension().and_then(|e| e.to_str()), Some("gz") | Some("data")) {
                return true;
            }
        }
        false
    }
    has_data_file(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coin_labels_match_the_archive_filename_prefix() {
        assert_eq!(Coin::Btc.label(), "btc");
        assert_eq!(Coin::Eth.label(), "eth");
        assert_eq!(Coin::Sol.label(), "sol");
    }

    #[test]
    fn builds_the_expected_zenodo_url_per_coin() {
        assert_eq!(Coin::Btc.url(), "https://zenodo.org/records/18184441/files/btc_orders_202512.tar.xz?download=1");
        assert_eq!(Coin::Eth.url(), "https://zenodo.org/records/18184441/files/eth_orders_202512.tar.xz?download=1");
        assert_eq!(Coin::Sol.url(), "https://zenodo.org/records/18184441/files/sol_orders_202512.tar.xz?download=1");
    }

    #[test]
    fn all_returns_every_coin_exactly_once() {
        let all = Coin::all();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&Coin::Btc));
        assert!(all.contains(&Coin::Eth));
        assert!(all.contains(&Coin::Sol));
    }

    #[test]
    fn dest_already_populated_detects_nested_gz_files() {
        let root = std::env::temp_dir().join(format!("market_sim_dl_test_{}", std::process::id()));
        let nested = root.join("20251201");
        fs::create_dir_all(&nested).unwrap();
        assert!(!dest_already_populated(&root), "empty tree should report unpopulated");

        fs::write(nested.join("btc_00.data.gz"), b"").unwrap();
        assert!(dest_already_populated(&root), "a .gz file nested under a date folder should be found");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dir_size_sums_nested_files_recursively() {
        let root = std::env::temp_dir().join(format!("market_sim_dirsize_test_{}", std::process::id()));
        let nested = root.join("20251201");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(dir_size(&root), 0, "empty tree should sum to zero");

        fs::write(root.join("a.txt"), b"12345").unwrap(); // 5 bytes
        fs::write(nested.join("b.txt"), b"1234567890").unwrap(); // 10 bytes
        assert_eq!(dir_size(&root), 15, "should sum files at every depth, not just the top level");

        fs::remove_dir_all(&root).ok();
    }

    /// End-to-end check against the real `tar`/`xz` on this machine: builds
    /// a synthetic multi-file `.tar.xz`, extracts it through the exact same
    /// `extract_archive` the `download`/`extract` commands call, and checks
    /// the result lands correctly — including that `xz_uncompressed_size`
    /// parses real `xz -l --robot` output. `#[ignore]`d by default since it
    /// shells out to real external tools rather than being a pure unit
    /// test, matching `inputs::simulator`'s own real-data integration test.
    /// Run explicitly with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn extract_archive_produces_the_expected_files() {
        let root = std::env::temp_dir().join(format!("market_sim_extract_test_{}", std::process::id()));
        let src = root.join("src");
        let dest = root.join("dest");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dest).unwrap();

        fs::write(src.join("f1.txt"), vec![b'a'; 10_000]).unwrap();
        fs::write(src.join("f2.txt"), vec![b'b'; 20_000]).unwrap();

        // Built in two steps rather than `tar -cJf` directly: Windows'
        // bundled bsdtar (what `Command::new("tar")` resolves to from a
        // native process — confirmed by hand: `tar -cJf` there fails with
        // "Unsupported compression option --xz") can only create plain
        // .tar, not compress while creating. It (and xz) can both do their
        // own half fine, which is all `extract_archive` itself ever
        // depends on anyway — it only ever extracts, never creates.
        let tar_file = root.join("test.tar");
        let status = Command::new("tar").arg("-cf").arg(&tar_file).arg("-C").arg(&src).arg(".").status().expect("tar should be runnable on this machine");
        assert!(status.success(), "building the synthetic .tar should succeed");

        let mut xz_cmd = Command::new("xz");
        if let Some(path) = path_with_bundled_xz() {
            xz_cmd.env("PATH", path);
        }
        let status = xz_cmd.arg("-z").arg(&tar_file).status().expect("xz should be runnable on this machine");
        assert!(status.success(), "compressing the synthetic .tar should succeed");
        let archive = root.join("test.tar.xz"); // xz -z replaces test.tar with test.tar.xz in place

        // Confirms xz's index-based size lookup works against a real xz
        // stream, not just that the function compiles.
        let total = xz_uncompressed_size(&archive).expect("xz -l --robot should report a size for a real archive");
        assert!(total >= 30_000, "uncompressed size should be at least the two files' combined size, got {total}");

        extract_archive("test", &archive, &dest, "https://example.invalid/unused-in-this-test");

        assert!(!archive.exists(), "archive should be removed after a successful extraction");
        assert_eq!(fs::read(dest.join("f1.txt")).unwrap().len(), 10_000);
        assert_eq!(fs::read(dest.join("f2.txt")).unwrap().len(), 20_000);

        fs::remove_dir_all(&root).ok();
    }
}
