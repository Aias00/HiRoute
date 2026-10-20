//! Paired writer acceptance accepts an actual captured suffix, never fabricated identities.
use crate::writer::{IngestOutcome, ObservationCommitPort};
use crate::{DigestAuthority, LocalObservationStore};
use hiroute_domain::{
    ConversationContentEnvelopeV1, ExecutionFactEnvelopeV1, ObservationAckV2,
    ObservationGapHeartbeatV1,
};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    time::Instant,
};
use zeroize::Zeroizing;

#[test]
#[ignore = "requires a marked stopped history and genuine captured native envelopes"]
fn captured_native_suffix_uses_the_real_writer() {
    let root = PathBuf::from(std::env::var_os("HIROUTE_STORAGE_FIELD_ROOT").expect("field root"));
    assert_eq!(root.file_name().unwrap(), "observation");
    assert!(
        root.parent()
            .unwrap()
            .join(".hiroute-storage-validation")
            .is_file()
    );
    let key = Zeroizing::new(
        fs::read(std::env::var_os("HIROUTE_STORAGE_FIELD_KEY_FILE").expect("key file")).unwrap(),
    );
    let store = LocalObservationStore::open(
        &root,
        DigestAuthority::new(key.as_slice().try_into().unwrap()),
    )
    .unwrap();
    let input =
        fs::File::open(std::env::var_os("HIROUTE_STORAGE_FIELD_REPLAY").expect("captured suffix"))
            .unwrap();
    let start = Instant::now();
    let mut events = 0u64;
    for line in BufReader::new(input).lines() {
        let line = Zeroizing::new(line.unwrap());
        let record: serde_json::Value = serde_json::from_str(&line).unwrap();
        let expected: ObservationAckV2 = serde_json::from_value(record["ack"].clone()).unwrap();
        let apply = || {
            if record["kind"] == "gap_heartbeat" {
                let heartbeat: ObservationGapHeartbeatV1 =
                    serde_json::from_value(record["envelope"].clone()).unwrap();
                return store.ingest_gap_heartbeat(&heartbeat).unwrap();
            }
            match record["channel"].as_str().unwrap() {
                "execution_fact" => {
                    let event: ExecutionFactEnvelopeV1 =
                        serde_json::from_value(record["envelope"].clone()).unwrap();
                    store.ingest_fact(&event, &[]).unwrap()
                }
                "conversation_content" => {
                    let event: ConversationContentEnvelopeV1 =
                        serde_json::from_value(record["envelope"].clone()).unwrap();
                    store.ingest_content(&event, &[]).unwrap()
                }
                _ => panic!("capture contains an unsupported carrier; do not rewrite or omit it"),
            }
        };
        assert_eq!(
            apply(),
            IngestOutcome::Ack(expected.clone()),
            "original first-ingest acknowledgement at event {events}"
        );
        assert_eq!(
            apply(),
            IngestOutcome::Ack(expected),
            "exact duplicate acknowledgement at event {events}"
        );
        events += 1;
    }
    assert!(events > 0, "zero captured events is not acceptance");
    println!(
        "{}",
        serde_json::json!({"phase":"native_suffix_replayed","events":events,"seconds":start.elapsed().as_secs_f64()})
    );
}
