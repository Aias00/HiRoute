//! Bounded peer admission and socket write progress, independent of request execution.
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use hyper::{body::Incoming, service::Service};
use std::{
    convert::Infallible,
    future::Future,
    io::{self, IoSlice},
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError},
    time::Sleep,
};

const CONNECTIONS_PER_SCOPE: usize = 64;
const WRITE_PROGRESS_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct Connections {
    loopback: Arc<Semaphore>,
    network: Arc<Semaphore>,
}
impl Default for Connections {
    fn default() -> Self {
        Self {
            loopback: Arc::new(Semaphore::new(CONNECTIONS_PER_SCOPE)),
            network: Arc::new(Semaphore::new(CONNECTIONS_PER_SCOPE)),
        }
    }
}
impl Connections {
    pub fn acquire(&self, peer: IpAddr) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        if peer.is_loopback() {
            &self.loopback
        } else {
            &self.network
        }
        .clone()
        .try_acquire_owned()
    }
}

pub(super) async fn serve<I, S>(stream: I, service: S) -> Result<(), hyper::Error>
where
    I: AsyncRead + AsyncWrite + Unpin,
    S: Service<Request<Incoming>, Response = Response<Full<Bytes>>, Error = Infallible>,
{
    hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .max_buf_size(64 * 1024)
        .serve_connection(
            hyper_util::rt::TokioIo::new(WriteProgress::new(stream)),
            service,
        )
        .await
}

struct WriteProgress<I> {
    inner: I,
    stalled: Option<Pin<Box<Sleep>>>,
}
impl<I> WriteProgress<I> {
    fn new(inner: I) -> Self {
        Self {
            inner,
            stalled: None,
        }
    }
    fn written(
        &mut self,
        cx: &mut Context<'_>,
        result: Poll<io::Result<usize>>,
    ) -> Poll<io::Result<usize>> {
        match result {
            Poll::Pending => {
                // Arm only when bytes cannot reach the socket, never while a service
                // is awaiting an operation or a user confirmation.
                let deadline = self
                    .stalled
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(WRITE_PROGRESS_TIMEOUT)));
                if deadline.as_mut().poll(cx).is_ready() {
                    Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "console response write made no progress",
                    )))
                } else {
                    Poll::Pending
                }
            }
            ready => {
                if matches!(ready, Poll::Ready(Ok(count)) if count > 0) {
                    self.stalled = None;
                }
                ready
            }
        }
    }
}
impl<I: AsyncRead + Unpin> AsyncRead for WriteProgress<I> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}
impl<I: AsyncWrite + Unpin> AsyncWrite for WriteProgress<I> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        this.written(cx, result)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write_vectored(cx, buffers);
        this.written(cx, result)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // TCP flush is a ready no-op, not evidence that pending response bytes moved.
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
