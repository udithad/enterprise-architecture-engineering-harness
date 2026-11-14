use tokio::io::{AsyncRead, AsyncReadExt};

/// Read a stream to EOF keeping at most `max` bytes; the remainder is drained and
/// discarded so the child never blocks on a full pipe.
pub(crate) async fn read_bounded<R: AsyncRead + Unpin>(mut r: R, max: usize) -> (String, bool) {
    let mut kept = Vec::with_capacity(max.min(64 * 1024));
    let mut buf = [0u8; 8192];
    let mut truncated = false;
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(kept.len());
                if room > 0 {
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
                if n > room {
                    truncated = true;
                }
            }
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}
