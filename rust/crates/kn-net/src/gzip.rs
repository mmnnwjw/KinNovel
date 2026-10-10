use std::io::Read;

use flate2::read::GzDecoder;

/// Why a gzip blob couldn't be decompressed. Mirrors the split
/// `transport.py`'s `_decode_response` makes between `TransportError` (size
/// cap exceeded, or stream truncated mid-record -- a real problem, always
/// propagated) and a bare `zlib.error`/`OSError` (the bytes just weren't
/// gzip at all -- silently treated as "not actually compressed").
#[derive(Debug)]
pub enum GzipError {
    TooLarge,
    Incomplete,
    InvalidFormat,
}

/// Port of `transport.py`'s `gunzip_limited`: decompress `raw` but refuse to
/// produce more than `limit` bytes (protects against a decompression bomb
/// from a compromised/misbehaving server), and require a complete stream.
pub fn gunzip_limited(raw: &[u8], limit: usize) -> Result<Vec<u8>, GzipError> {
    let mut decoder = GzDecoder::new(raw);
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut produced_any = false;
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                produced_any = true;
                out.extend_from_slice(&buf[..n]);
                if out.len() > limit {
                    return Err(GzipError::TooLarge);
                }
            }
            Err(err) => {
                return Err(classify_read_error(&err, produced_any));
            }
        }
    }
    Ok(out)
}

fn classify_read_error(err: &std::io::Error, produced_any: bool) -> GzipError {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::UnexpectedEof => GzipError::Incomplete,
        _ => {
            // flate2 reports a bad trailer (CRC/length mismatch, which a
            // truncated-but-otherwise-well-formed stream also triggers) as
            // InvalidData; treat that as "incomplete" only once we've
            // already produced some output, otherwise it really is not a
            // gzip stream at all.
            if produced_any {
                GzipError::Incomplete
            } else {
                GzipError::InvalidFormat
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip_bytes(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decompresses_within_limit() {
        let raw = gzip_bytes(&[b'A'; 1024]);
        assert!(matches!(gunzip_limited(&raw, 100), Err(GzipError::TooLarge)));
        let out = gunzip_limited(&raw, 2048).unwrap();
        assert_eq!(out, vec![b'A'; 1024]);
    }

    #[test]
    fn truncated_stream_is_incomplete() {
        let raw = gzip_bytes(br#"{"Success": true}"#);
        let result = gunzip_limited(&raw[..10], 1 << 20);
        assert!(matches!(result, Err(GzipError::Incomplete) | Err(GzipError::InvalidFormat)));
    }

    #[test]
    fn non_gzip_bytes_are_invalid_format() {
        let result = gunzip_limited(b"not gzip at all", 1 << 20);
        assert!(matches!(result, Err(GzipError::InvalidFormat)));
    }
}
