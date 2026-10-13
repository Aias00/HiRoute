//! Operating-system primitives shared by headless hosts.
//!
//! Business authorization and lifecycle policy remain with their existing owners.
//! Windows FFI is isolated here; callers use owned handles and checked operations.

#[cfg(windows)]
pub mod windows;
