use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use tokio::io::AsyncRead;

use crate::logger;

static POLL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Wraps an AsyncRead and logs every complete line read through it.
/// Also logs Pending polls periodically to diagnose polling gaps.
pub struct LoggingReader<R> {
    inner: R,
    line_buf: Vec<u8>,
    pending_since: Option<std::time::Instant>,
}

impl<R> LoggingReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line_buf: Vec::new(),
            pending_since: None,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LoggingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let poll_num = POLL_COUNT.fetch_add(1, Ordering::Relaxed);
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);

        match &result {
            Poll::Pending => {
                let me = &mut *self;
                if me.pending_since.is_none() {
                    me.pending_since = Some(std::time::Instant::now());
                    logger::log_to_file(&format!("STDIN POLL #{poll_num}: Pending (start)"));
                }
            }
            Poll::Ready(Ok(())) => {
                let new_bytes_len = buf.filled().len() - before;
                let me = &mut *self;

                // Log gap if we were pending
                if let Some(since) = me.pending_since.take() {
                    let gap_ms = since.elapsed().as_millis();
                    if gap_ms > 10 {
                        logger::log_to_file(&format!(
                            "STDIN POLL #{poll_num}: Ready after {gap_ms}ms pending gap"
                        ));
                    }
                }

                if new_bytes_len > 0 {
                    let new_bytes = buf.filled()[before..].to_vec();
                    me.line_buf.extend_from_slice(&new_bytes);

                    // Flush complete lines
                    while let Some(pos) = me.line_buf.iter().position(|&b| b == b'\n') {
                        let line = &me.line_buf[..pos];
                        if let Ok(s) = std::str::from_utf8(line) {
                            logger::log_to_file(&format!("RAW RECV: {s}"));
                        }
                        me.line_buf.drain(..=pos);
                    }
                }
            }
            Poll::Ready(Err(e)) => {
                logger::log_to_file(&format!("STDIN POLL #{poll_num}: Error: {e}"));
            }
        }

        result
    }
}
