//! Installed extensions: the list the user manages, kept on disk and handed
//! to the web engine.

use std::sync::Arc;

use domain::{Extension, ExtensionId, Extensions};

use crate::ports::{ExtensionError, ExtensionPackages, Repository, RepositoryError, WebEngine};

pub struct ExtensionManager {
    extensions: Extensions,
    repository: Box<dyn Repository<Extensions>>,
    packages: Arc<dyn ExtensionPackages>,
}

impl ExtensionManager {
    /// Loads the saved list; files of extensions no longer in it are deleted.
    pub fn load(
        repository: Box<dyn Repository<Extensions>>,
        packages: Arc<dyn ExtensionPackages>,
    ) -> Result<Self, RepositoryError> {
        let extensions = repository.load()?;
        packages.prune(&extensions);
        Ok(Self {
            extensions,
            repository,
            packages,
        })
    }

    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    /// The installer, for a background thread (installing downloads).
    pub fn packages(&self) -> Arc<dyn ExtensionPackages> {
        self.packages.clone()
    }

    /// Records an extension the installer unpacked, and runs it.
    pub fn installed(
        &mut self,
        extension: Extension,
        engine: &mut dyn WebEngine,
    ) -> Result<(), RepositoryError> {
        self.extensions.install(extension);
        self.changed(engine)
    }

    pub fn set_enabled(
        &mut self,
        id: &ExtensionId,
        enabled: bool,
        engine: &mut dyn WebEngine,
    ) -> Result<(), RepositoryError> {
        if self.extensions.set_enabled(id, enabled) {
            self.changed(engine)?;
        }
        Ok(())
    }

    /// Stops the extension, then deletes its files.
    pub fn remove(
        &mut self,
        id: &ExtensionId,
        engine: &mut dyn WebEngine,
    ) -> Result<(), ExtensionError> {
        if !self.extensions.remove(id) {
            return Ok(());
        }
        self.changed(engine)
            .map_err(|e| ExtensionError(e.to_string()))?;
        self.packages.remove(id)
    }

    fn changed(&mut self, engine: &mut dyn WebEngine) -> Result<(), RepositoryError> {
        engine.set_extensions(&self.extensions);
        self.repository.save(&self.extensions)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use domain::{TabId, WebAddress};

    use super::*;
    use crate::ports::{EngineError, ExtensionSource, Viewport};

    const ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    #[derive(Default)]
    struct Engine {
        running: Vec<(String, bool)>,
    }

    impl WebEngine for Engine {
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
        fn set_extensions(&mut self, extensions: &Extensions) {
            self.running = extensions
                .items()
                .iter()
                .map(|e| (e.name.clone(), e.enabled))
                .collect();
        }
    }

    #[derive(Default, Clone)]
    struct Store(Arc<Mutex<Option<Extensions>>>);

    impl Repository<Extensions> for Store {
        fn load(&self) -> Result<Extensions, RepositoryError> {
            Ok(self.0.lock().unwrap().clone().unwrap_or_default())
        }
        fn save(&self, value: &Extensions) -> Result<(), RepositoryError> {
            *self.0.lock().unwrap() = Some(value.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Packages {
        removed: Mutex<Vec<String>>,
        pruned: Mutex<usize>,
    }

    impl ExtensionPackages for Packages {
        fn install(&self, _: &ExtensionSource) -> Result<Extension, ExtensionError> {
            unreachable!()
        }
        fn remove(&self, id: &ExtensionId) -> Result<(), ExtensionError> {
            self.removed.lock().unwrap().push(id.to_string());
            Ok(())
        }
        fn prune(&self, _: &Extensions) {
            *self.pruned.lock().unwrap() += 1;
        }
    }

    fn extension() -> Extension {
        Extension {
            id: ExtensionId::parse(ID).unwrap(),
            name: "uBO Lite".into(),
            version: "1".into(),
            description: String::new(),
            enabled: true,
            popup: None,
            options: None,
        }
    }

    #[test]
    fn changes_reach_the_engine_and_the_disk() {
        let store = Store::default();
        let packages = Arc::new(Packages::default());
        let mut engine = Engine::default();
        let mut manager =
            ExtensionManager::load(Box::new(store.clone()), packages.clone()).unwrap();
        assert_eq!(*packages.pruned.lock().unwrap(), 1);

        manager.installed(extension(), &mut engine).unwrap();
        assert_eq!(engine.running, vec![("uBO Lite".into(), true)]);
        let id = ExtensionId::parse(ID).unwrap();
        manager.set_enabled(&id, false, &mut engine).unwrap();
        assert_eq!(engine.running, vec![("uBO Lite".into(), false)]);
        assert!(!store.load().unwrap().items()[0].enabled);

        manager.remove(&id, &mut engine).unwrap();
        assert!(engine.running.is_empty());
        assert!(store.load().unwrap().is_empty());
        assert_eq!(*packages.removed.lock().unwrap(), vec![ID.to_string()]);
    }
}
