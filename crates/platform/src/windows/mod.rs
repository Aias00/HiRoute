//! Windows kernel boundaries. No borrowed numeric handle escapes this module.
pub(crate) mod durable;
pub(crate) mod file_security;
pub mod background;
pub mod job;
pub mod launcher;
pub mod lifecycle;
mod path;
pub mod pipe;
pub mod security;
pub mod sealed_host;
pub mod service;
pub mod task;
