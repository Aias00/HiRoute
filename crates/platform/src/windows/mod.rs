//! Windows kernel boundaries. No borrowed numeric handle escapes this module.
pub mod durable;
pub mod diagnostic_files;
pub(crate) mod file_security;
pub mod background;
pub mod job;
pub mod launcher;
pub mod lifecycle;
mod path;
pub mod pipe;
pub mod security;
pub mod sealed_host;
pub mod scm_owner;
pub mod service;
pub mod task;
