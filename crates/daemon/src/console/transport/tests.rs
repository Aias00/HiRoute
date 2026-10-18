use super::*;
use hiroute_product_session::web_confirmation::{ConfirmationBroker, WebConfirmationPrompt};
use hyper::service::service_fn;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream},
    task::JoinHandle,
};

fn connect<S>(
    connections: &Connections,
    peer: &str,
    service: S,
) -> (
    BufReader<DuplexStream>,
    JoinHandle<Result<(), hyper::Error>>,
)
where
    S: Service<Request<Incoming>, Response = Response<Full<Bytes>>, Error = Infallible>
        + Send
        + 'static,
    S::Future: Send,
{
    // Deliberately small buffers make socket backpressure deterministic. The
    // production Hyper owner, write watchdog and permit lifetime remain intact.
    let (client, server) = tokio::io::duplex(1024);
    let permit = connections.acquire(peer.parse().unwrap()).unwrap();
    let task = tokio::spawn(async move {
        let _permit = permit;
        serve(server, service).await
    });
    (BufReader::new(client), task)
}

async fn send(client: &mut BufReader<DuplexStream>, path: &str) {
    client
        .get_mut()
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .await
        .unwrap();
}

async fn response(client: &mut BufReader<DuplexStream>) -> Vec<u8> {
    let mut line = String::new();
    client.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("HTTP/1.1 200 "), "{line:?}");
    let mut length = None;
    loop {
        line.clear();
        assert_ne!(client.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut bytes = vec![0; length.expect("full response has a known length")];
    client.read_exact(&mut bytes).await.unwrap();
    bytes
}

#[test]
fn peer_pools_have_independent_limits_and_release_on_drop() {
    let connections = Connections::default();
    let network = (0..CONNECTIONS_PER_SCOPE)
        .map(|_| connections.acquire("192.0.2.1".parse().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(connections.acquire("2001:db8::1".parse().unwrap()).is_err());
    let loopback = (0..CONNECTIONS_PER_SCOPE)
        .map(|index| {
            let peer = if index % 2 == 0 { "127.0.0.1" } else { "::1" };
            connections.acquire(peer.parse().unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    assert!(connections.acquire("127.0.0.1".parse().unwrap()).is_err());
    drop(network);
    assert!(connections.acquire("192.0.2.1".parse().unwrap()).is_ok());
    assert!(connections.acquire("::1".parse().unwrap()).is_err());
    drop(loopback);
    assert!(connections.acquire("::1".parse().unwrap()).is_ok());
}

#[tokio::test(start_paused = true)]
async fn stalled_network_responses_leave_loopback_usable_and_release_their_permits() {
    let connections = Connections::default();
    let mut clients = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..CONNECTIONS_PER_SCOPE {
        let (mut client, task) = connect(
            &connections,
            "192.0.2.1",
            service_fn(|_| async {
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(vec![b'x'; 64 * 1024]))))
            }),
        );
        send(&mut client, "/bundle.js").await;
        clients.push(client); // No response bytes are read from these peers.
        tasks.push(task);
    }
    tokio::task::yield_now().await;
    assert_eq!(connections.network.available_permits(), 0);
    assert!(connections.acquire("192.0.2.2".parse().unwrap()).is_err());

    let (mut local, local_task) = connect(
        &connections,
        "127.0.0.1",
        service_fn(|_| async {
            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"local ready"))))
        }),
    );
    send(&mut local, "/").await;
    assert_eq!(response(&mut local).await, b"local ready");
    drop(local);
    local_task.await.unwrap().unwrap();

    tokio::time::advance(WRITE_PROGRESS_TIMEOUT + Duration::from_secs(1)).await;
    for task in tasks {
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("stalled write must close without the peer disconnecting")
            .unwrap();
        assert!(result.is_err(), "stalled write must close");
    }
    assert_eq!(
        connections.network.available_permits(),
        CONNECTIONS_PER_SCOPE
    );
    assert_eq!(clients.len(), CONNECTIONS_PER_SCOPE);
    // Reclamation happened while all unread peers were still open.
    assert!(connections.acquire("192.0.2.2".parse().unwrap()).is_ok());
}

#[tokio::test(start_paused = true)]
async fn successful_byte_progress_refreshes_the_write_deadline() {
    let (mut reader, writer) = tokio::io::duplex(1);
    let writer = tokio::spawn(async move { WriteProgress::new(writer).write_all(b"abcdef").await });
    tokio::task::yield_now().await;
    let mut received = Vec::new();
    for _ in 0..6 {
        tokio::time::advance(Duration::from_secs(20)).await;
        received.push(reader.read_u8().await.unwrap());
        tokio::task::yield_now().await;
    }
    // Total transfer time exceeds both the progress timeout and the former
    // connection lifetime, but each stalled interval is shorter than 30 s.
    writer.await.unwrap().unwrap();
    assert_eq!(received, b"abcdef");
}

#[tokio::test(start_paused = true)]
async fn ready_flush_does_not_refresh_a_stalled_vectored_write() {
    let (_reader, writer) = tokio::io::duplex(1);
    let mut writer = WriteProgress::new(writer);
    writer.write_all(b"x").await.unwrap();
    let buffers = [IoSlice::new(b"y"), IoSlice::new(b"z")];
    let poll_write = |writer: &mut WriteProgress<DuplexStream>| {
        let mut context = Context::from_waker(std::task::Waker::noop());
        Pin::new(writer).poll_write_vectored(&mut context, &buffers)
    };
    assert!(poll_write(&mut writer).is_pending());
    tokio::time::advance(Duration::from_secs(29)).await;
    writer.flush().await.unwrap();
    tokio::time::advance(Duration::from_secs(2)).await;
    match poll_write(&mut writer) {
        Poll::Ready(Err(error)) => assert_eq!(error.kind(), io::ErrorKind::TimedOut),
        result => panic!("a no-op flush cannot extend a stalled write: {result:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn aged_keep_alive_keeps_the_full_confirmation_window() {
    let connections = Connections::default();
    let broker = Arc::new(ConfirmationBroker::default());
    let confirmations = broker.clone();
    let (mut client, task) = connect(
        &connections,
        "::1",
        service_fn(move |request: Request<Incoming>| {
            let broker = confirmations.clone();
            async move {
                let body = if request.uri().path() == "/confirm" {
                    assert!(
                        broker
                            .request(
                                "owner",
                                WebConfirmationPrompt {
                                    title: "Apply verified change".into(),
                                    message: "Confirm the exact previewed intent.".into(),
                                    confirm_label: "Apply".into(),
                                    cancel_label: "Cancel".into(),
                                }
                            )
                            .await
                    );
                    b"accepted".as_slice()
                } else {
                    b"ready".as_slice()
                };
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(body))))
            }
        }),
    );
    send(&mut client, "/").await;
    assert_eq!(response(&mut client).await, b"ready");
    for _ in 0..12 {
        tokio::time::advance(Duration::from_secs(9)).await;
        send(&mut client, "/").await;
        assert_eq!(response(&mut client).await, b"ready");
    }
    send(&mut client, "/confirm").await;
    tokio::task::yield_now().await;
    let prompt = broker
        .snapshot("owner")
        .expect("service is awaiting confirmation");
    tokio::time::advance(Duration::from_secs(59)).await;
    assert!(
        !task.is_finished(),
        "confirmation wait must not arm a write timer"
    );
    assert!(broker.snapshot("owner").is_some());
    broker
        .resolve("owner", &prompt.confirmation_id, true)
        .unwrap();
    assert_eq!(response(&mut client).await, b"accepted");
    send(&mut client, "/").await;
    assert_eq!(response(&mut client).await, b"ready");
    drop(client);
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn idle_headers_still_expire_and_release_the_connection() {
    let connections = Connections::default();
    let (client, task) = connect(
        &connections,
        "127.0.0.1",
        service_fn(|_| async {
            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ready"))))
        }),
    );
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        connections.loopback.available_permits(),
        CONNECTIONS_PER_SCOPE
    );
    drop(client);
}
