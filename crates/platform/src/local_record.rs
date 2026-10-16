//! Bounded one-request records for local transports without write-half close.
//! Business payloads and schemas belong to their existing owners. End-of-record
//! is explicit; bytes within a record must be completely parsed by that owner.

use std::io::{self, Read, Write};
use zeroize::Zeroizing;

pub const CONTROL_ACK: &[u8] = b"HRACK1\n";
const MAGIC: &[u8; 8] = b"HRPIPE1\0";
const END: &[u8; 4] = b"END1";

#[derive(Clone, Copy)]
pub enum Channel {
    ProtectedInput = 1,
    AgentGrant = 2,
    Lifecycle = 3,
}
#[derive(Clone, Copy)]
pub enum Direction {
    Request = 1,
    Response = 2,
}

pub fn write(
    stream: &mut impl Write,
    channel: Channel,
    direction: Direction,
    body: &[u8],
    maximum: usize,
) -> io::Result<()> {
    if body.is_empty() || body.len() > maximum {
        return Err(invalid());
    }
    let length = u32::try_from(body.len()).map_err(|_| invalid())?;
    let mut header = [0_u8; 14];
    header[..8].copy_from_slice(MAGIC);
    header[8] = channel as u8;
    header[9] = direction as u8;
    header[10..].copy_from_slice(&length.to_be_bytes());
    stream.write_all(&header)?;
    stream.write_all(body)?;
    stream.write_all(END)?;
    stream.flush()
}

pub fn read(
    stream: &mut impl Read,
    channel: Channel,
    direction: Direction,
    maximum: usize,
) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut header = [0_u8; 14];
    stream.read_exact(&mut header)?;
    if &header[..8] != MAGIC || header[8] != channel as u8 || header[9] != direction as u8 {
        return Err(invalid());
    }
    let length = u32::from_be_bytes(header[10..14].try_into().map_err(|_| invalid())?) as usize;
    if length == 0 || length > maximum {
        return Err(invalid());
    }
    let mut body = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut body)?;
    let mut end = [0; 4];
    stream.read_exact(&mut end)?;
    if &end != END {
        return Err(invalid());
    }
    Ok(body)
}

pub fn acknowledge(stream: &mut impl Write) -> io::Result<()> {
    stream.write_all(CONTROL_ACK)?;
    stream.flush()
}

pub fn await_acknowledgement(stream: &mut impl Read) -> io::Result<()> {
    let mut acknowledgement = [0; 7];
    stream.read_exact(&mut acknowledgement)?;
    if acknowledgement != CONTROL_ACK {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid bounded local record")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_wrong_channel_direction_truncation_end_and_oversize() {
        let mut record = Vec::new();
        write(
            &mut record,
            Channel::AgentGrant,
            Direction::Request,
            b"synthetic",
            9,
        )
        .unwrap();
        assert_eq!(
            &*read(&mut &record[..], Channel::AgentGrant, Direction::Request, 9).unwrap(),
            b"synthetic"
        );
        assert!(
            read(
                &mut &record[..],
                Channel::ProtectedInput,
                Direction::Request,
                9
            )
            .is_err()
        );
        assert!(
            read(
                &mut &record[..],
                Channel::AgentGrant,
                Direction::Response,
                9
            )
            .is_err()
        );
        assert!(read(&mut &record[..], Channel::AgentGrant, Direction::Request, 8).is_err());
        assert!(
            read(
                &mut &record[..record.len() - 1],
                Channel::AgentGrant,
                Direction::Request,
                9
            )
            .is_err()
        );
        *record.last_mut().unwrap() = b'X';
        assert!(read(&mut &record[..], Channel::AgentGrant, Direction::Request, 9).is_err());
    }
}
