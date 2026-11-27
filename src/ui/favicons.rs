//! The tabs' page icons, as textures for the tab strip, and whether they
//! play sound. Each site's icon is also kept on disk, small: a tab asleep
//! (restored, or put to sleep) shows it without loading its page.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use domain::{Session, TabId};

/// Icons are shown at most this big (pixels): a larger one is scaled down
/// before it becomes a texture or is written.
const MAX_SIDE: u32 = 32;

/// Each tab's icon, once its page gave one.
#[derive(Default)]
pub struct Favicons {
    /// Where the sites' icons are kept (`<host>.png`); `None`: nowhere (a
    /// private window).
    dir: Option<PathBuf>,
    /// The kept icons read so far, by host (`None`: there is none), for
    /// the open tabs' sites only.
    by_host: HashMap<String, Option<egui::TextureHandle>>,
    icons: HashMap<TabId, egui::TextureHandle>,
    /// Arrived from the engine, not made into textures yet.
    pending: Vec<(TabId, Vec<u8>)>,
    /// The tabs that play sound or are muted: (playing, muted).
    audio: HashMap<TabId, (bool, bool)>,
    /// The tabs and their addresses when last looked at: unchanged, there is
    /// nothing to look for.
    seen: Option<u64>,
    /// Writes the icons kept, off the UI thread.
    writer: Option<Writer>,
}

/// A thread that writes the icons it is sent.
struct Writer {
    tx: Option<Sender<(PathBuf, image::RgbaImage)>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel::<(PathBuf, image::RgbaImage)>();
        let thread = std::thread::spawn(move || {
            for (path, image) in rx {
                write_icon(&path, &image);
            }
        });
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    fn write(&self, path: PathBuf, image: image::RgbaImage) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((path, image));
        }
    }
}

impl Drop for Writer {
    /// The icons sent are written before it goes.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Tells the tabs and their addresses apart, without copying them.
fn tabs_fingerprint(session: &Session) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for tab in session.tabs() {
        (tab.id(), tab.address().map(|a| a.as_str())).hash(&mut hasher);
    }
    hasher.finish()
}

/// What a tab's sound button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Playing,
    Muted,
}

impl Favicons {
    /// Icons kept in `dir` (`None`: kept nowhere).
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            ..Self::default()
        }
    }

    /// `tab`'s new icon (a PNG; empty: none).
    pub fn changed(&mut self, tab: TabId, png: Vec<u8>) {
        self.pending.push((tab, png));
    }

    /// Makes the textures of the icons that arrived (and keeps them for
    /// their site); gives the tabs without one their site's kept icon;
    /// forgets closed tabs'.
    pub fn update(&mut self, ctx: &egui::Context, session: &Session) {
        let seen = tabs_fingerprint(session);
        if self.pending.is_empty() && self.seen == Some(seen) {
            return;
        }
        self.seen = Some(seen);
        for (tab, png) in std::mem::take(&mut self.pending) {
            match decode(&png) {
                Some(image) => {
                    let texture = ctx.load_texture(
                        format!("favicon {}", tab.get()),
                        texture_image(&image),
                        egui::TextureOptions::LINEAR,
                    );
                    if let Some(host) = host_of(session, tab) {
                        self.keep(&host, image);
                        self.by_host.insert(host, Some(texture.clone()));
                    }
                    self.icons.insert(tab, texture);
                }
                None => {
                    self.icons.remove(&tab);
                }
            }
        }
        // Only the open tabs' sites stay in memory.
        let hosts: HashSet<String> = session
            .tabs()
            .iter()
            .filter_map(|t| host_of(session, t.id()))
            .collect();
        self.by_host.retain(|host, _| hosts.contains(host));
        // Tabs without an icon yet (asleep, or loading): their site's.
        for tab in session.tabs() {
            if self.icons.contains_key(&tab.id()) {
                continue;
            }
            let Some(host) = host_of(session, tab.id()) else {
                continue;
            };
            if let Some(texture) = self.kept(ctx, &host) {
                self.icons.insert(tab.id(), texture);
            }
        }
        self.icons.retain(|id, _| session.tab(*id).is_some());
        self.audio.retain(|id, _| session.tab(*id).is_some());
    }

    /// `tab`'s page plays sound or not, muted or not.
    pub fn audio(&mut self, tab: TabId, playing: bool, muted: bool) {
        if playing || muted {
            self.audio.insert(tab, (playing, muted));
        } else {
            self.audio.remove(&tab);
        }
    }

    /// The sound button `tab` shows, if any: muted ones keep it to be
    /// unmuted.
    pub fn sound(&self, tab: TabId) -> Option<Sound> {
        match self.audio.get(&tab)? {
            (_, true) => Some(Sound::Muted),
            (true, false) => Some(Sound::Playing),
            (false, false) => None,
        }
    }

    pub fn get(&self, tab: TabId) -> Option<&egui::TextureHandle> {
        self.icons.get(&tab)
    }

    /// Writes `host`'s icon in the background, when it changed.
    fn keep(&mut self, host: &str, image: image::RgbaImage) {
        let Some(path) = self.path(host) else { return };
        self.writer
            .get_or_insert_with(Writer::start)
            .write(path, image);
    }

    /// `host`'s kept icon, read once.
    fn kept(&mut self, ctx: &egui::Context, host: &str) -> Option<egui::TextureHandle> {
        if let Some(known) = self.by_host.get(host) {
            return known.clone();
        }
        // With or without "www.": a site often redirects from one to the
        // other, its icon kept under the address it ended on.
        let other = host
            .strip_prefix("www.")
            .map_or_else(|| format!("www.{host}"), str::to_owned);
        let texture = [host, other.as_str()]
            .iter()
            .find_map(|h| self.path(h).and_then(|p| std::fs::read(p).ok()))
            .and_then(|png| decode(&png))
            .map(|image| {
                ctx.load_texture(
                    format!("favicon {host}"),
                    texture_image(&image),
                    egui::TextureOptions::LINEAR,
                )
            });
        self.by_host.insert(host.to_owned(), texture.clone());
        texture
    }

    /// Where `host`'s icon is kept (a plain host name only).
    fn path(&self, host: &str) -> Option<PathBuf> {
        let plain = !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            && !host.starts_with('.');
        let dir = self.dir.as_ref().filter(|_| plain)?;
        Some(dir.join(format!("{host}.png")))
    }
}

/// The host of `tab`'s page (not a private tab's: nothing kept of those).
fn host_of(session: &Session, tab: TabId) -> Option<String> {
    let tab = session.tab(tab).filter(|t| !t.is_private())?;
    tab.address()?.host().map(str::to_ascii_lowercase)
}

/// The icon in `png`, at most [`MAX_SIDE`] pixels on its longer side.
fn decode(png: &[u8]) -> Option<image::RgbaImage> {
    if png.is_empty() {
        return None;
    }
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let image = if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
        image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    Some(image.into_rgba8())
}

fn texture_image(image: &image::RgbaImage) -> egui::ColorImage {
    let size = [image.width() as usize, image.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
}

/// Writes the icon at `path` as a PNG, unless the same is there.
fn write_icon(path: &std::path::Path, image: &image::RgbaImage) {
    let mut png = Vec::new();
    if let Err(e) = image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png) {
        log::warn!("favicon {}: {e}", path.display());
        return;
    }
    if std::fs::read(path).is_ok_and(|kept| kept == png) {
        return;
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, &png));
    if let Err(e) = written {
        log::warn!("favicon {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pngs_decode_and_garbage_does_not() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode(&png).unwrap().dimensions(), (2, 3));
        assert!(decode(b"not a png").is_none());
        assert!(decode(&[]).is_none());
    }

    #[test]
    fn a_large_icon_is_scaled_down() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(256, 128, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode(&png).unwrap().dimensions(), (32, 16));
    }

    #[test]
    fn only_the_open_tabs_sites_stay_in_memory() {
        let ctx = egui::Context::default();
        let mut session = Session::new();
        let tab = session.open_tab();
        let page = domain::WebAddress::parse("https://example.com/").unwrap();
        session.tab_mut(tab).unwrap().load_started(page);
        let mut icons = Favicons::new(None);
        icons.changed(tab, png());
        icons.update(&ctx, &session);
        assert!(icons.by_host.contains_key("example.com"));
        let other = domain::WebAddress::parse("https://other.example/").unwrap();
        session.tab_mut(tab).unwrap().load_started(other);
        icons.update(&ctx, &session);
        assert!(!icons.by_host.contains_key("example.com"));
    }

    fn png() -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn a_sites_icon_is_kept_for_its_tabs_asleep() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let page = domain::WebAddress::parse("https://www.example.com/a").unwrap();

        let mut session = Session::new();
        let tab = session.open_tab();
        session.tab_mut(tab).unwrap().load_started(page.clone());
        let mut icons = Favicons::new(Some(dir.path().to_path_buf()));
        icons.changed(tab, png());
        icons.update(&ctx, &session);
        // Gone: what it was writing is written.
        drop(icons);
        assert!(dir.path().join("www.example.com.png").exists());

        // Next run: the tab restored asleep shows it, without its page.
        let mut session = Session::new();
        let asleep = session.open_tab();
        session.tab_mut(asleep).unwrap().restore(page, "Example");
        // Saved without "www.": found all the same.
        let other = session.open_tab();
        let bare = domain::WebAddress::parse("https://example.com/").unwrap();
        session.tab_mut(other).unwrap().restore(bare, "");
        let mut icons = Favicons::new(Some(dir.path().to_path_buf()));
        icons.update(&ctx, &session);
        assert!(icons.get(asleep).is_some());
        assert!(icons.get(other).is_some());
    }

    #[test]
    fn nothing_is_kept_without_a_folder_or_for_odd_hosts() {
        let icons = Favicons::new(None);
        assert!(icons.path("example.com").is_none());
        let icons = Favicons::new(Some(PathBuf::from("x")));
        assert!(icons.path("../evil").is_none());
        assert!(icons.path("").is_none());
        assert_eq!(
            icons.path("a-b.com"),
            Some(PathBuf::from("x").join("a-b.com.png"))
        );
    }
}
