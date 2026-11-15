//! Application layer: the browser's use cases.
//!
//! [`Browser`] drives the domain and talks to the outside world only through
//! the ports in [`ports`], which the infrastructure implements. Nothing here
//! knows about a window, a web engine or a file format.

mod browser;
mod error;
pub mod extensions;
pub mod ports;
pub mod sync;
pub mod user_scripts;

pub use browser::{Browser, Target};
pub use error::BrowserError;
pub use extensions::ExtensionManager;
pub use user_scripts::UserScriptManager;
