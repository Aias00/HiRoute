use super::*;
use std::io::{self, Cursor, Read};

struct ShortReads<'a> {
    inner: Cursor<&'a [u8]>,
    max_requested: usize,
    interrupt_next: bool,
}

impl Read for ShortReads<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.max_requested = self.max_requested.max(buffer.len());
        if self.interrupt_next {
            self.interrupt_next = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.interrupt_next = true;
        let length = buffer.len().min(997);
        self.inner.read(&mut buffer[..length])
    }
}

#[test]
fn recovery_digest_streams_large_files_with_the_original_digest() {
    let bytes: Vec<u8> = (0..(17 * 64 * 1024 + 37))
        .map(|index| (index % 251) as u8)
        .collect();
    let expected = CanonicalDigest::of_bytes(&bytes);
    let mut reader = ShortReads {
        inner: Cursor::new(bytes.as_slice()),
        max_requested: 0,
        interrupt_next: true,
    };
    assert_eq!(
        stream_digest(&mut reader, bytes.len() as u64).unwrap(),
        expected
    );
    assert!(reader.max_requested <= 64 * 1024);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.sqlite");
    fs::write(&path, &bytes).unwrap();
    assert_eq!(file_digest(&path).unwrap(), expected);
    fs::write(&path, b"changed source").unwrap();
    assert_ne!(file_digest(&path).unwrap(), expected);
}

#[test]
fn recovery_digest_rejects_truncated_or_growing_source() {
    assert!(stream_digest(&mut Cursor::new(b"short"), 6).is_err());
    assert!(stream_digest(&mut Cursor::new(b"growing"), 6).is_err());
    assert_eq!(
        stream_digest(&mut Cursor::new([]), 0).unwrap(),
        CanonicalDigest::of_bytes(b"")
    );
}

struct ReadFailure;

impl Read for ReadFailure {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

#[test]
fn recovery_digest_preserves_read_failures() {
    assert!(stream_digest(&mut ReadFailure, 1).is_err());
    assert!(stream_digest(&mut ReadFailure, 0).is_err());
}
