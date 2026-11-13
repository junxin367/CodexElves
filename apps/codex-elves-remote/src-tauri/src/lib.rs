pub mod appearance;
pub mod connection;
pub mod discovery;
pub mod resource_cache;
pub mod settings;

#[cfg(windows)]
pub mod app_bar;

#[cfg(windows)]
pub mod desktop;

#[cfg(windows)]
pub mod native_cache;

#[cfg(windows)]
pub mod taskbar;
