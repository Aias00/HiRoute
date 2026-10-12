//! Stable identity shared by the management producer and protected Desktop input preparation.

use crate::{CanonicalDigest, CanonicalDigestError};

/// Preserve the released v2 identity: source ownership and the protected candidate reference,
/// without its transient revision. Knowing this ID grants no credential authority.
pub fn managed_key_id(
    source_id: &str,
    candidate_ref: &str,
) -> Result<String, CanonicalDigestError> {
    let digest = CanonicalDigest::of(&(
        "hiroute.compute-management-key/v2",
        source_id,
        candidate_ref,
    ))?;
    Ok(format!(
        "credential/managed-{}",
        super::digest_suffix(&digest, 24)
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn released_key_identity_keeps_its_canonical_bytes_and_source_scope() {
        assert_eq!(
            super::managed_key_id("source/test", "candidate/test").unwrap(),
            "credential/managed-7be9f916223c72e005f2f164"
        );
        assert_ne!(
            super::managed_key_id("source/other", "candidate/test").unwrap(),
            super::managed_key_id("source/test", "candidate/test").unwrap()
        );
    }
}
