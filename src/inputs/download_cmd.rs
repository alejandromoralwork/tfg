//! The `download` command: fetches BTC/ETH/SOL order-status archives from
//! the same Zenodo record `data/download_data.sh` already uses
//! (<https://zenodo.org/records/18184441>), skipping the `_rejected`
//! variants (the engines never need them — see `Order::is_new_live_order`),
//! and extracts them ready for `simulate <btc|eth|sol>` to consume.
//!
//! Shells out to `curl` and `tar` rather than adding an HTTP client + LZMA
//! decoder as real Cargo dependencies — both ship natively on this machine
//! (`C:\Windows\system32\curl.exe`/`tar.exe`, Windows 10 1803+), and
//! `download_data.sh` already relies on exactly the same two tools for
//! exactly this dataset. Consistent with why this project added `flate2`
//! only after concluding hand-rolling gzip wasn't reasonable — the same
//! logic argues against hand-rolling curl+tar here.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn download_one(coin: Coin) {
    let label = coin.label();
    let data_root = Path::new("data");
    let dest = data_root.join("order_statuses").join(label);

    if let Err(err) = fs::create_dir_all(&dest) {
        println!("❌ Failed to create '{}': {err}", dest.display());
        return;
    }

    if dest_already_populated(&dest) {
        println!("✅ '{}' already has data — skipping download (delete it first to re-fetch).", dest.display());
        return;
    }

    let archive = data_root.join(coin.archive_filename());
    let url = coin.url();

    println!("⬇️  Downloading {label} order-status data from Zenodo ...");
    println!("   {url}");
    std::io::stdout().flush().ok();

    // -L: follow redirects. -C -: resume a partial download (or skip the
    // transfer entirely if the file is already complete). --fail: nonzero
    // exit on an HTTP error instead of writing the error page to the
    // archive. --retry 3: same resilience download_data.sh already relies
    // on for these same multi-GB files.
    let curl_status = Command::new("curl").args(["-L", "-C", "-", "--fail", "--retry", "3", "-o"]).arg(&archive).arg(&url).status();

    match curl_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("❌ curl exited with {status} — download failed or the archive doesn't exist at that URL.");
            return;
        }
        Err(err) => {
            println!("❌ Couldn't run 'curl' ({err}). Is curl.exe on PATH?");
            return;
        }
    }

    println!("📦 Extracting {} -> {}/ ...", archive.display(), dest.display());
    std::io::stdout().flush().ok();

    let tar_status = Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&dest).status();

    match tar_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("❌ tar exited with {status} — extraction failed. Archive left at '{}' for a manual retry.", archive.display());
            return;
        }
        Err(err) => {
            println!("❌ Couldn't run 'tar' ({err}). Is tar.exe on PATH? Archive left at '{}'.", archive.display());
            return;
        }
    }

    // Multi-GB archives — not worth keeping once extracted, matching
    // download_data.sh's default behavior.
    if let Err(err) = fs::remove_file(&archive) {
        println!("⚠️  Extracted OK, but couldn't remove archive '{}': {err}", archive.display());
    }

    println!("✅ {label} order-status data ready at '{}'. Try: simulate {label}", dest.display());
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
}
