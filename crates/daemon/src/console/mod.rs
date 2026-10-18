//! Authenticated product sessions. The standalone composition owns listener lifetime.
mod assets;
pub mod auth;
pub(crate) mod authority;
mod contexts;
pub mod control;
mod host_settings;
mod http;
mod server;
mod transport;
pub use server::ConsoleService;

#[cfg(all(test, unix))]
mod tests;
