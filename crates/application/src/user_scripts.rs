//! Installed user scripts: the list the user manages, kept on disk and
//! handed to the web engine.

use std::sync::Arc;

use domain::{UserScript, UserScriptId, UserScripts};

use crate::ports::{ExtensionError, Repository, RepositoryError, UserScriptPackages, WebEngine};

pub struct UserScriptManager {
    scripts: UserScripts,
    repository: Box<dyn Repository<UserScripts>>,
    packages: Arc<dyn UserScriptPackages>,
}

impl UserScriptManager {
    /// Loads the saved list; code of scripts no longer in it is deleted.
    pub fn load(
        repository: Box<dyn Repository<UserScripts>>,
        packages: Arc<dyn UserScriptPackages>,
    ) -> Result<Self, RepositoryError> {
        let scripts = repository.load()?;
        packages.prune(&scripts);
        Ok(Self {
            scripts,
            repository,
            packages,
        })
    }

    pub fn scripts(&self) -> &UserScripts {
        &self.scripts
    }

    /// The installer, for a background thread (installing downloads).
    pub fn packages(&self) -> Arc<dyn UserScriptPackages> {
        self.packages.clone()
    }

    /// Records a script the installer saved, and runs it.
    pub fn installed(
        &mut self,
        script: UserScript,
        engine: &mut dyn WebEngine,
    ) -> Result<(), RepositoryError> {
        self.scripts.install(script);
        self.changed(engine)
    }

    pub fn set_enabled(
        &mut self,
        id: &UserScriptId,
        enabled: bool,
        engine: &mut dyn WebEngine,
    ) -> Result<(), RepositoryError> {
        if self.scripts.set_enabled(id, enabled) {
            self.changed(engine)?;
        }
        Ok(())
    }

    pub fn remove(
        &mut self,
        id: &UserScriptId,
        engine: &mut dyn WebEngine,
    ) -> Result<(), ExtensionError> {
        if !self.scripts.remove(id) {
            return Ok(());
        }
        self.changed(engine)
            .map_err(|e| ExtensionError(e.to_string()))?;
        self.packages.remove(id)
    }

    fn changed(&mut self, engine: &mut dyn WebEngine) -> Result<(), RepositoryError> {
        engine.set_user_scripts(&self.scripts);
        self.repository.save(&self.scripts)
    }
}
