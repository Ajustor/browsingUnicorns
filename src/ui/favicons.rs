//! The tabs' page icons, as textures for the tab strip.

use std::collections::HashMap;

use domain::{Session, TabId};

/// Each tab's icon, once its page gave one.
#[derive(Default)]
pub struct Favicons {
    icons: HashMap<TabId, egui::TextureHandle>,
    /// Arrived from the engine, not made into textures yet.
    pending: Vec<(TabId, Vec<u8>)>,
}

impl Favicons {
    /// `tab`'s new icon (a PNG; empty: none).
    pub fn changed(&mut self, tab: TabId, png: Vec<u8>) {
        self.pending.push((tab, png));
    }

    /// Makes the textures of the icons that arrived; forgets closed tabs'.
    pub fn update(&mut self, ctx: &egui::Context, session: &Session) {
        for (tab, png) in std::mem::take(&mut self.pending) {
            match decode(&png) {
                Some(image) => {
                    let texture = ctx.load_texture(
                        format!("favicon {}", tab.get()),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.icons.insert(tab, texture);
                }
                None => {
                    self.icons.remove(&tab);
                }
            }
        }
        self.icons.retain(|id, _| session.tab(*id).is_some());
    }

    pub fn get(&self, tab: TabId) -> Option<&egui::TextureHandle> {
        self.icons.get(&tab)
    }
}

fn decode(png: &[u8]) -> Option<egui::ColorImage> {
    if png.is_empty() {
        return None;
    }
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
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
        assert_eq!(decode(&png).unwrap().size, [2, 3]);
        assert!(decode(b"not a png").is_none());
        assert!(decode(&[]).is_none());
    }
}
