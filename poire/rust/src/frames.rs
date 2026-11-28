//! RS-05 cancellation-unsafe reads, and RS-23 unbounded ingress.

use std::fmt;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

/// The longest frame body this reader accepts, in bytes.
pub const MAX_FRAME: u32 = 64 * 1024;

/// Why a frame could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum FrameError {
    /// The peer announced a body longer than [`MAX_FRAME`].
    TooLong(u32),
    /// The stream ended inside a frame.
    Truncated,
    /// The stream failed.
    Io(io::Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong(n) => write!(f, "frame of {n} bytes exceeds {MAX_FRAME}"),
            Self::Truncated => f.write_str("stream ended inside a frame"),
            Self::Io(_) => f.write_str("frame read failed"),
        }
    }
}

impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::TooLong(_) | Self::Truncated => None,
        }
    }
}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Reads frames of a big-endian `u32` length followed by that many bytes.
///
/// Bytes that arrive before a frame is complete are kept in the reader, not in
/// the future. Dropping a pending [`read_frame`](Self::read_frame), as
/// `tokio::select!` does to a losing branch, therefore loses nothing.
#[derive(Debug, Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    /// An empty reader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the next frame, or `None` at a clean end of stream.
    ///
    /// Cancellation-safe: if the future is dropped before it completes, the next
    /// call continues where this one stopped. The length is checked as soon as
    /// its four bytes arrive, so an oversized frame is refused before its body
    /// is buffered.
    ///
    /// # Errors
    /// [`FrameError::TooLong`] for a length over [`MAX_FRAME`],
    /// [`FrameError::Truncated`] if the stream ends mid-frame,
    /// [`FrameError::Io`] if a read fails.
    pub async fn read_frame<R>(&mut self, src: &mut R) -> Result<Option<Vec<u8>>, FrameError>
    where
        R: AsyncRead + Unpin,
    {
        loop {
            if let Some(frame) = self.take_frame()? {
                return Ok(Some(frame));
            }
            // `read_buf` is cancellation-safe: it either appends what it read or
            // nothing, and `self.buf` outlives this future.
            if src.read_buf(&mut self.buf).await? == 0 {
                return if self.buf.is_empty() {
                    Ok(None)
                } else {
                    Err(FrameError::Truncated)
                };
            }
        }
    }

    fn take_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let Some((head, rest)) = self.buf.split_first_chunk::<4>() else {
            return Ok(None);
        };
        let announced = u32::from_be_bytes(*head);
        if announced > MAX_FRAME {
            return Err(FrameError::TooLong(announced));
        }
        let Ok(len) = usize::try_from(announced) else {
            return Err(FrameError::TooLong(announced));
        };
        let Some(body) = rest.get(..len) else {
            return Ok(None);
        };
        let body = body.to_vec();
        self.buf.drain(..4 + len);
        Ok(Some(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn frame(body: &[u8]) -> Vec<u8> {
        let len = u32::try_from(body.len()).unwrap();
        let mut out = len.to_be_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn a_cancelled_read_loses_nothing() {
        let (mut tx, mut rx) = tokio::io::duplex(64);
        let whole = frame(b"0123456789");
        let (first, second) = whole.split_at(6);
        tx.write_all(first).await.unwrap();

        let mut reader = FrameReader::new();
        tokio::select! {
            biased;
            _ = reader.read_frame(&mut rx) => unreachable!("half a frame cannot complete"),
            () = std::future::ready(()) => {}
        }

        tx.write_all(second).await.unwrap();
        drop(tx);
        let got = reader.read_frame(&mut rx).await.unwrap();
        assert_eq!(
            got.as_deref(),
            Some(&b"0123456789"[..]),
            "the frame survives the cancel"
        );
    }

    /// The cancellation-unsafe shape of RS-05: partial bytes live in the future.
    async fn naive_read(src: &mut tokio::io::DuplexStream) -> io::Result<Vec<u8>> {
        let mut head = [0u8; 4];
        src.read_exact(&mut head).await?;
        let mut body = vec![0u8; usize::try_from(u32::from_be_bytes(head)).unwrap()];
        src.read_exact(&mut body).await?;
        Ok(body)
    }

    #[tokio::test]
    async fn the_naive_reader_loses_bytes_on_cancel() {
        let (mut tx, mut rx) = tokio::io::duplex(64);
        // Bytes 2..6 of the body read as a length of 2 once the frame is torn.
        let body = [b'x', b'y', 0, 0, 0, 2, b'p', b'q', b'r', b's'];
        let whole = frame(&body);
        let (first, second) = whole.split_at(6);
        tx.write_all(first).await.unwrap();
        tokio::select! {
            biased;
            _ = naive_read(&mut rx) => unreachable!("half a frame cannot complete"),
            () = std::future::ready(()) => {}
        }
        tx.write_all(second).await.unwrap();
        let got = naive_read(&mut rx).await.unwrap();
        assert_eq!(got, b"pq", "the torn frame is misread as a new one");
    }

    #[tokio::test]
    async fn refuses_an_oversized_length_before_its_body() {
        let (mut tx, mut rx) = tokio::io::duplex(64);
        tx.write_all(&(MAX_FRAME + 1).to_be_bytes()).await.unwrap();
        drop(tx);
        let got = FrameReader::new().read_frame(&mut rx).await;
        assert!(
            matches!(got, Err(FrameError::TooLong(n)) if n == MAX_FRAME + 1),
            "the length alone is enough to refuse"
        );
    }

    #[tokio::test]
    async fn accepts_a_frame_of_exactly_the_limit() {
        let limit = usize::try_from(MAX_FRAME).unwrap();
        let (mut tx, mut rx) = tokio::io::duplex(limit + 8);
        tx.write_all(&frame(&vec![7; limit])).await.unwrap();
        drop(tx);
        let got = FrameReader::new().read_frame(&mut rx).await.unwrap();
        assert_eq!(got.map(|b| b.len()), Some(65_536), "64 KiB is allowed");
    }

    proptest::proptest! {
        #[test]
        fn property_a_frame_survives_any_cut(
            body in proptest::collection::vec(proptest::num::u8::ANY, 0..256),
            cut in 0_usize..300,
        ) {
            let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let got = runtime.block_on(async {
                let whole = frame(&body);
                let (first, second) = whole.split_at(cut.min(whole.len()));
                let (mut tx, mut rx) = tokio::io::duplex(1024);
                tx.write_all(first).await.unwrap();
                let mut reader = FrameReader::new();
                // Poll once on the bytes so far; if that cannot finish, cancel it.
                let early = tokio::select! {
                    biased;
                    got = reader.read_frame(&mut rx) => Some(got.unwrap()),
                    () = std::future::ready(()) => None,
                };
                tx.write_all(second).await.unwrap();
                drop(tx);
                match early {
                    Some(frame) => frame,
                    None => reader.read_frame(&mut rx).await.unwrap(),
                }
            });
            proptest::prop_assert_eq!(got, Some(body));
        }
    }

    #[test]
    fn errors_say_what_happened() {
        let io = FrameError::from(io::Error::other("reset"));
        assert!(
            std::error::Error::source(&io).is_some(),
            "an I/O error keeps its cause"
        );
        assert!(
            std::error::Error::source(&FrameError::Truncated).is_none(),
            "no cause"
        );
        assert_eq!(
            FrameError::TooLong(70_000).to_string(),
            "frame of 70000 bytes exceeds 65536",
            "names both numbers"
        );
    }

    #[tokio::test]
    async fn reports_truncation_and_clean_end() {
        let (mut tx, mut rx) = tokio::io::duplex(64);
        tx.write_all(&frame(b"ab")).await.unwrap();
        tx.write_all(&[0, 0]).await.unwrap();
        drop(tx);
        let mut reader = FrameReader::new();
        let first = reader.read_frame(&mut rx).await.unwrap();
        assert_eq!(first.as_deref(), Some(&b"ab"[..]), "a whole frame reads");
        let rest = reader.read_frame(&mut rx).await;
        assert!(
            matches!(rest, Err(FrameError::Truncated)),
            "two stray bytes are a truncated frame"
        );

        let (tx, mut rx) = tokio::io::duplex(8);
        drop(tx);
        let none = FrameReader::new().read_frame(&mut rx).await.unwrap();
        assert!(none.is_none(), "an empty stream is a clean end");
    }
}
