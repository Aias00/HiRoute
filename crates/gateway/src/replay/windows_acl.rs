//! Shared Windows owner-only security and held file identity.
pub(crate) use hiroute_platform::windows::security::{
    create_owner_only_directory, create_owner_only_file, delete_by_handle, file_identity,
    set_owner_only, verify_owner_only, verify_owner_only_handle,
};
