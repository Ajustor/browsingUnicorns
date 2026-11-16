//! What the pages download: the engine saves it in the user's Downloads
//! folder, the notice line tells and offers to open it.

use std::path::Path;
use std::process::Command;

use application::ports::EngineEvent;

use super::updates::Notice;

/// The notice for a download event, if `event` is one.
pub fn notice(event: &EngineEvent) -> Option<Notice> {
    Some(match event {
        EngineEvent::DownloadStarted { file } => Notice {
            error: false,
            text: format!("Downloading {}…", name(file)),
            file: None,
        },
        EngineEvent::DownloadFinished {
            file: Some(file), ..
        } => Notice {
            error: false,
            text: format!("{} downloaded.", name(file)),
            file: Some(file.clone()),
        },
        EngineEvent::DownloadFinished { url, file: None } => Notice {
            error: true,
            text: format!("Download failed: {url}"),
            file: None,
        },
        _ => return None,
    })
}

fn name(file: &Path) -> String {
    file.file_name().map_or_else(
        || file.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Opens `file` with its application.
pub fn open(file: &Path) {
    #[cfg(windows)]
    let result = Command::new("explorer").arg(file).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(file).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(file).spawn();
    if let Err(e) = result {
        log::warn!("open {}: {e}", file.display());
    }
}

/// Shows `file` in its folder.
pub fn reveal(file: &Path) {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt as _;
        // explorer wants `/select,"<path>"` as one raw argument.
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg("-R").arg(file).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open")
        .arg(
            file.parent()
                .map_or_else(|| std::path::PathBuf::from("."), Path::to_path_buf),
        )
        .spawn();
    if let Err(e) = result {
        log::warn!("reveal {}: {e}", file.display());
    }
}
