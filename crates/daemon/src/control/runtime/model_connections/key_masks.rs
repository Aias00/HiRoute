//! Bounded display masks generated under the coordinated Secret authority.

use hiroute_application::control::ComputeManagementControlError as Error;
use hiroute_application_api::{
    ComputeKeyMaskViewV1, ComputeKeyMasksQueryV1, ComputeKeyMasksViewV1,
};
use hiroute_domain::{ComputeManagementRepositoryPort, SecretStorePort, VerifiedSecretSubjectV1};

impl super::LocalControlAdapter {
    pub(super) fn saved_key_masks(
        &self,
        query: &ComputeKeyMasksQueryV1,
    ) -> Result<ComputeKeyMasksViewV1, Error> {
        let stores = self.stores_lock().map_err(|_| Error::Unavailable)?;
        let source = stores
            .control()
            .compute_management_source(&query.source_id)
            .map_err(super::map_port)?
            .ok_or(Error::NotFound)?;
        if source.revision != query.expected_source_revision {
            return Err(Error::Conflict);
        }
        if !source.provenance.is_native() {
            return Err(Error::Invalid);
        }
        let subject = VerifiedSecretSubjectV1::from_authenticated_transport(
            "hirouted",
            format!("source/{}", source.source_id),
        )
        .map_err(|_| Error::Corrupt)?;
        let destination = source
            .target
            .credential_destination()
            .map_err(|_| Error::Corrupt)?;
        let mut keys = Vec::with_capacity(source.credentials.len());
        for key in &source.credentials {
            let generation = key.credential.generation();
            let secret = stores
                .secrets()
                .resolve_secret(
                    &subject,
                    &key.credential,
                    "provider-auth",
                    &destination,
                    generation,
                )
                .map_err(super::map_port)?;
            let value = std::str::from_utf8(secret.expose()).map_err(|_| Error::Corrupt)?;
            let masked_key = mask(value);
            keys.push(ComputeKeyMaskViewV1 {
                key_id: key.key_id.clone(),
                generation,
                masked_key,
            });
        }
        Ok(ComputeKeyMasksViewV1 {
            source_id: source.source_id,
            source_revision: source.revision,
            keys,
        })
    }
}

fn mask(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    if chars.iter().any(|ch| ch.is_control()) || chars.len() <= 2 {
        return "••••••••".into();
    }
    if chars.len() <= 12 {
        return format!(
            "••••••••{}",
            chars[chars.len() - 2..].iter().collect::<String>()
        );
    }
    format!(
        "{}••••••••{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn masks_never_reveal_short_keys_and_bound_long_keys() {
        for value in ["a", "ab", "abc", "123456789012"] {
            assert!(!super::mask(value).contains(value));
        }
        assert_eq!(super::mask("sk-test-secret-value-9f03"), "sk-t••••••••9f03");
        assert_eq!(super::mask("test\ncontrol-key"), "••••••••");
    }
}
