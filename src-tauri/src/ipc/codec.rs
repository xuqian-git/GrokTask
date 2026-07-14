//! UTF-8 NDJSON codec with frame size limits.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Error, ErrorKind};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Default max frame size: 8 MiB (persistence-ipc §7).
pub const DEFAULT_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Snapshot chunk budget: 1 MiB.
pub const SNAPSHOT_CHUNK_MAX_BYTES: usize = 1024 * 1024;

/// Large-item raw fragment budget before base64: 700 KiB.
pub const SNAPSHOT_FRAGMENT_RAW_MAX: usize = 700 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame exceeds limit ({got} > {limit} bytes)")]
    FrameTooLarge { got: usize, limit: usize },
    #[error("invalid UTF-8 in frame")]
    InvalidUtf8,
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("connection closed (EOF)")]
    Eof,
}

impl From<CodecError> for std::io::Error {
    fn from(e: CodecError) -> Self {
        match e {
            CodecError::Io(e) => e,
            CodecError::Eof => Error::new(ErrorKind::UnexpectedEof, "EOF"),
            other => Error::new(ErrorKind::InvalidData, other.to_string()),
        }
    }
}

/// Write one NDJSON message (JSON + newline) and flush.
pub async fn write_msg<W, T>(w: &mut W, msg: &T) -> Result<(), CodecError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    write_msg_limited(w, msg, DEFAULT_MAX_FRAME_BYTES).await
}

pub async fn write_msg_limited<W, T>(w: &mut W, msg: &T, max_frame: usize) -> Result<(), CodecError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut line = serde_json::to_vec(msg).map_err(|e| CodecError::InvalidJson(e.to_string()))?;
    if line.len() > max_frame {
        return Err(CodecError::FrameTooLarge {
            got: line.len(),
            limit: max_frame,
        });
    }
    line.push(b'\n');
    w.write_all(&line).await?;
    w.flush().await?;
    Ok(())
}

/// Read the next non-empty NDJSON line and deserialize.
/// Empty lines are skipped. EOF returns `Ok(None)`.
pub async fn read_msg<R, T>(r: &mut R) -> Result<Option<T>, CodecError>
where
    R: AsyncBufRead + Unpin,
    T: DeserializeOwned,
{
    read_msg_limited(r, DEFAULT_MAX_FRAME_BYTES).await
}

pub async fn read_msg_limited<R, T>(r: &mut R, max_frame: usize) -> Result<Option<T>, CodecError>
where
    R: AsyncBufRead + Unpin,
    T: DeserializeOwned,
{
    loop {
        let mut line = Vec::new();
        let n = read_line_limited(r, &mut line, max_frame).await?;
        if n == 0 {
            return Ok(None);
        }
        // Trim trailing newline(s) and whitespace.
        while line
            .last()
            .is_some_and(|b| matches!(b, b'\n' | b'\r' | b' ' | b'\t'))
        {
            line.pop();
        }
        if line.is_empty() {
            continue; // skip empty lines
        }
        let text = std::str::from_utf8(&line).map_err(|_| CodecError::InvalidUtf8)?;
        let msg = serde_json::from_str(text).map_err(|e| CodecError::InvalidJson(e.to_string()))?;
        return Ok(Some(msg));
    }
}

/// Read until `\n` with a hard byte cap (does not rely on String growth alone).
async fn read_line_limited<R: AsyncBufRead + Unpin>(
    r: &mut R,
    buf: &mut Vec<u8>,
    max_frame: usize,
) -> Result<usize, CodecError> {
    buf.clear();
    loop {
        let available = r.fill_buf().await?;
        if available.is_empty() {
            return Ok(buf.len()); // EOF
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            if buf.len() + take > max_frame + 1 {
                return Err(CodecError::FrameTooLarge {
                    got: buf.len() + take,
                    limit: max_frame,
                });
            }
            buf.extend_from_slice(&available[..take]);
            r.consume(take);
            return Ok(buf.len());
        }
        // No newline yet — consume whole buffer slice.
        let take = available.len();
        if buf.len() + take > max_frame {
            return Err(CodecError::FrameTooLarge {
                got: buf.len() + take,
                limit: max_frame,
            });
        }
        buf.extend_from_slice(available);
        r.consume(take);
    }
}

/// Encode a JSON value to NDJSON bytes (for tests / size checks).
pub fn encode_line<T: Serialize>(msg: &T) -> Result<Vec<u8>, CodecError> {
    let mut line = serde_json::to_vec(msg).map_err(|e| CodecError::InvalidJson(e.to_string()))?;
    line.push(b'\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::BinaryFingerprint;
    use crate::ipc::protocol::{ClientRole, Hello};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn roundtrip() {
        let (mut tx, rx) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(rx);
        let sent = Hello::new(
            "r1",
            ClientRole::Mcp,
            "0.1.0",
            "/bin/GrokTask",
            BinaryFingerprint {
                size: 10,
                mtime_ns: 20,
            },
            7,
        );
        write_msg(&mut tx, &sent).await.unwrap();
        let got: Option<Hello> = read_msg(&mut reader).await.unwrap();
        assert_eq!(got.unwrap().pid, 7);
    }

    #[tokio::test]
    async fn eof_returns_none() {
        let (tx, rx) = tokio::io::duplex(16);
        drop(tx);
        let mut reader = BufReader::new(rx);
        let got: Option<Hello> = read_msg(&mut reader).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn empty_lines_skipped() {
        let (mut tx, rx) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(rx);
        tx.write_all(b"\n\n").await.unwrap();
        let msg = Hello::new(
            "r",
            ClientRole::Cli,
            "0.1.0",
            "/x",
            BinaryFingerprint::ZERO,
            1,
        );
        write_msg(&mut tx, &msg).await.unwrap();
        drop(tx);
        let got: Option<Hello> = read_msg(&mut reader).await.unwrap();
        assert!(got.is_some());
    }

    #[tokio::test]
    async fn invalid_json_errors() {
        let (mut tx, rx) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(rx);
        tx.write_all(b"{not-json}\n").await.unwrap();
        drop(tx);
        let err = read_msg::<_, Hello>(&mut reader).await.unwrap_err();
        assert!(matches!(err, CodecError::InvalidJson(_)));
    }

    #[tokio::test]
    async fn oversize_frame_rejected() {
        let (mut tx, rx) = tokio::io::duplex(64 * 1024);
        let mut reader = BufReader::new(rx);
        // Write more than 64-byte limit without newline until limit hits.
        let limit = 64;
        let big = vec![b'x'; limit + 10];
        // send without newline in chunks via write_all of oversize line
        let mut line = big;
        line.push(b'\n');
        tx.write_all(&line).await.unwrap();
        drop(tx);
        let err = read_msg_limited::<_, serde_json::Value>(&mut reader, limit)
            .await
            .unwrap_err();
        assert!(matches!(err, CodecError::FrameTooLarge { .. }));
    }

    #[tokio::test]
    async fn write_rejects_oversize_encode() {
        let (mut tx, _rx) = tokio::io::duplex(1024);
        let huge = "y".repeat(100);
        let err = write_msg_limited(&mut tx, &huge, 16).await.unwrap_err();
        assert!(matches!(err, CodecError::FrameTooLarge { .. }));
    }
}
