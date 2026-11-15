//! Installs an extension into a profile, as the Extensions panel does:
//!
//! ```sh
//! cargo run -p infrastructure --example install_extension -- <profile dir> <Web Store link, id or .crx>
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use application::ExtensionManager;
use application::ports::{EngineError, ExtensionPackages, ExtensionSource, Viewport, WebEngine};
use domain::{ExtensionId, TabId, WebAddress};
use infrastructure::JsonRepository;
use infrastructure::extensions::ExtensionFolder;

/// No views here: the browser hands the list to its engine at start-up.
struct NoEngine;

impl WebEngine for NoEngine {
    fn open_view(&mut self, _: TabId, _: &WebAddress) -> Result<(), EngineError> {
        Ok(())
    }
    fn close_view(&mut self, _: TabId) {}
    fn show(&mut self, _: Option<TabId>) {}
    fn navigate(&mut self, _: TabId, _: &WebAddress) -> Result<(), EngineError> {
        Ok(())
    }
    fn go_back(&mut self, _: TabId) {}
    fn go_forward(&mut self, _: TabId) {}
    fn reload(&mut self, _: TabId) {}
    fn set_viewport(&mut self, _: Viewport) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(profile), Some(input)) = (args.next(), args.next()) else {
        return Err("usage: install_extension <profile dir> <link, id or .crx>".into());
    };
    let profile = PathBuf::from(profile);
    let source = if input.ends_with(".crx") {
        ExtensionSource::File(input.into())
    } else {
        ExtensionSource::Store(ExtensionId::from_store_input(&input).ok_or("not an extension id")?)
    };
    let packages = Arc::new(ExtensionFolder::new(profile.join("extensions")));
    let mut manager = ExtensionManager::load(
        Box::new(JsonRepository::new(profile.join("extensions.json"))),
        packages.clone(),
    )?;
    let extension = packages.install(&source)?;
    println!(
        "{} {} ({})",
        extension.name, extension.version, extension.id
    );
    manager.installed(extension, &mut NoEngine)?;
    Ok(())
}
