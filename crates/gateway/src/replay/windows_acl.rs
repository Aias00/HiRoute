//! Shared Windows owner-only security and held file identity.
#[cfg(test)]
pub(crate) use hiroute_platform::windows::security::verify_owner_only;
pub(crate) use hiroute_platform::windows::security::{
    create_owner_only_directory, delete_by_handle, file_identity, set_owner_only,
    verify_owner_only_handle,
};
