//! Windows kernel boundaries. No borrowed numeric handle escapes this module.
pub mod background;
pub mod diagnostic_files;
pub mod durable;
pub(crate) mod file_security;
pub mod job;
pub mod launcher;
pub mod lifecycle;
mod path;
pub mod pipe;
mod read_lease;
pub mod scm_owner;
pub mod sealed_host;
pub mod security;
pub mod service;
pub mod task;
