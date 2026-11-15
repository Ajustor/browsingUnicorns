//! Lists what a sync group holds, decrypted (a debugging aid).
//!
//! ```sh
//! cargo run -p infrastructure --example sync_inspect -- <relay url> <sync code>
//! ```

use application::ports::SyncStore;
use infrastructure::sync::{EncryptedSyncStore, HttpTransport, SyncKey};

fn main() {
    let mut args = std::env::args().skip(1);
    if std::env::args().nth(1).as_deref() == Some("--new-code") {
        println!("{}", SyncKey::generate().code());
        return;
    }
    let (Some(relay), Some(code)) = (args.next(), args.next()) else {
        eprintln!("usage: sync_inspect <relay url> <sync code>");
        std::process::exit(2);
    };
    let Some(key) = SyncKey::from_code(&code) else {
        eprintln!("invalid sync code");
        std::process::exit(2);
    };
    let store = EncryptedSyncStore::new(key, HttpTransport::new(&relay));
    match store.pull() {
        Ok(records) => {
            println!("{} device(s)", records.len());
            for r in records {
                println!(
                    "- {} ({}): {} bookmarks, {} visits, {} tabs, home {}",
                    r.name,
                    r.device,
                    r.bookmarks.len(),
                    r.history.len(),
                    r.tabs.len(),
                    r.settings.home_page
                );
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
