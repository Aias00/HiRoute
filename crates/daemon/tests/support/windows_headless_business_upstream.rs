//! Independent bounded Native-source HTTP observations, separate from public CLI effects.
use super::{MODEL, SECRET};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Default)]
struct Ledger {
    posts: usize,
    gets: usize,
    rejected: usize,
    malformed: usize,
}
pub(super) struct Upstream {
    pub(super) address: SocketAddr,
    ledger: Arc<Mutex<Ledger>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Upstream {
    pub(super) fn diagnostic(&self) {
        let ledger = self.ledger.lock().unwrap();
        eprintln!(
            "Windows Native oracle: posts={}, gets={}, rejected={}, malformed={}",
            ledger.posts, ledger.gets, ledger.rejected, ledger.malformed
        );
    }
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let ledger = Arc::new(Mutex::new(Ledger::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_ledger = Arc::clone(&ledger);
        let worker_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Err(error) = serve(&mut stream, &worker_ledger) {
                            eprintln!(
                                "Windows Native oracle transport failure: io-kind={:?}, native-code={:?}",
                                error.kind(),
                                error.raw_os_error()
                            );
                            worker_ledger.lock().unwrap().malformed += 1;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => {
                        worker_ledger.lock().unwrap().malformed += 1;
                        break;
                    }
                }
            }
        });
        Self {
            address,
            ledger,
            stop,
            thread: Some(thread),
        }
    }
    pub(super) fn assert_requests(&self, posts: usize, gets: usize) {
        let ledger = self.ledger.lock().unwrap();
        assert_eq!(
            ledger.rejected, 0,
            "upstream must receive only the exact protected/committed credential"
        );
        assert_eq!(
            ledger.malformed, 0,
            "upstream must receive the exact bounded public protocol"
        );
        assert_eq!(ledger.posts, posts, "unexpected actual model POST count");
        assert_eq!(
            ledger.gets, gets,
            "unexpected actual authenticated saved-source GET count"
        );
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: &mut TcpStream, ledger: &Mutex<Ledger>) -> std::io::Result<()> {
    // Accepted Winsock streams inherit a nonblocking listener. Timeouts alone
    // cannot make the bounded request/body reads wait for their bytes.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid bounded fixture request",
        )
    };
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let mut buffer = [0; 512];
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&buffer[..count]);
    };
    if end > 8192 {
        return Err(invalid());
    }
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid())?;
    let mut lines = header.split("\r\n");
    let line = lines.next().ok_or_else(invalid)?;
    let method = line.split_whitespace().next().ok_or_else(invalid)?;
    let path = line.split_whitespace().nth(1).ok_or_else(invalid)?;
    let mut length = 0;
    let mut authenticated = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse::<usize>().map_err(|_| invalid())?;
        }
        if name.eq_ignore_ascii_case("authorization") {
            authenticated = value.trim() == format!("Bearer {SECRET}");
        }
    }
    if length > 65536 {
        return Err(invalid());
    }
    let method = method.to_owned();
    let path = path.to_owned();
    while bytes.len() - end < length {
        let mut buffer = [0; 512];
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if !authenticated {
        ledger.lock().unwrap().rejected += 1;
        return reply(stream, 401, json!({"error":"fixture credential rejected"}));
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/models") => {
            ledger.lock().unwrap().gets += 1;
            reply(
                stream,
                200,
                json!({"object":"list","data":[{"id":MODEL,"object":"model"}]}),
            )
        }
        ("POST", "/v1/responses") => {
            let request: Value =
                serde_json::from_slice(&bytes[end..end + length]).map_err(|_| invalid())?;
            if request["model"] != MODEL
                || request["stream"] != false
                || request["tools"][0]["name"] != "hiroute_probe"
            {
                return Err(invalid());
            }
            ledger.lock().unwrap().posts += 1;
            reply(
                stream,
                200,
                json!({"id":"resp_windows_probe","object":"response","status":"completed","model":MODEL,
                "output":[{"type":"function_call","call_id":"call_windows_probe","name":"hiroute_probe","arguments":"{\"value\":\"ok\"}"}],
                "usage":{"input_tokens":4,"output_tokens":3,"total_tokens":7}}),
            )
        }
        _ => Err(invalid()),
    }
}
fn reply(stream: &mut TcpStream, status: u16, value: Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(&value).unwrap();
    write!(
        stream,
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_source_oracle_reads_delayed_header_and_body_from_nonblocking_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let body = serde_json::to_vec(
                &json!({"model":MODEL,"stream":false,"tools":[{"name":"hiroute_probe"}]}),
            )
            .unwrap();
            thread::sleep(Duration::from_millis(50));
            write!(stream, "POST /v1/responses HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {SECRET}\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
            thread::sleep(Duration::from_millis(25));
            stream.write_all(&body).unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200 Fixture\r\n"));
            let end = response
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap()
                + 4;
            let result: Value = serde_json::from_slice(&response[end..]).unwrap();
            assert_eq!(result["output"][0]["name"], "hiroute_probe");
            assert_eq!(result["usage"]["total_tokens"], 7);
        });
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("native oracle accept failed: {error}"),
            }
        };
        // Make the Winsock inherited-mode precondition explicit on every OS so
        // the regression also catches an omitted normalization on developer hosts.
        stream.set_nonblocking(true).unwrap();
        let ledger = Mutex::new(Ledger::default());
        serve(&mut stream, &ledger).unwrap();
        drop(stream);
        client.join().unwrap();
        let ledger = ledger.lock().unwrap();
        assert_eq!(
            (ledger.posts, ledger.gets, ledger.rejected, ledger.malformed),
            (1, 0, 0, 0)
        );
    }
}
