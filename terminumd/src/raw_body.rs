// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A raw file's answer: streamed in chunks, never read whole, with a single `Range`.

use std::io::{Read, Seek, SeekFrom};

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;

const CHUNK: usize = 64 << 10;

/// What a `Range` header asks of a file of `len` bytes: `Ok(None)` the whole file (no header,
/// several ranges, or a unit other than bytes), `Ok(Some((start, end)))` inclusive, `Err(())` 416.
pub fn range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Ok(None) };
    if spec.contains(',') {
        return Ok(None);
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = if a.is_empty() {
        // a suffix: the last `b` bytes
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 || len == 0 {
            return Err(());
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let start: u64 = a.parse().map_err(|_| ())?;
        let end = if b.is_empty() { u64::MAX } else { b.parse().map_err(|_| ())? };
        if start >= len || end < start {
            return Err(());
        }
        (start, end.min(len - 1))
    };
    Ok(Some((start, end)))
}

/// The answer for an opened file of `len` bytes: 200 whole or 206 with `Content-Range`, the
/// body read `CHUNK` bytes at a time on the blocking pool as the client takes it. It seeks:
/// call it off the async workers.
pub fn response(mut f: std::fs::File, len: u64, range_header: Option<&str>) -> Response {
    let (status, start, count) = match range(range_header, len) {
        Err(()) => {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
            res.headers_mut().insert(header::CONTENT_RANGE, hv(format!("bytes */{len}")));
            return res;
        }
        Ok(None) => (StatusCode::OK, 0, len),
        Ok(Some((s, e))) => (StatusCode::PARTIAL_CONTENT, s, e - s + 1),
    };
    if let Err(e) = f.seek(SeekFrom::Start(start)) {
        let stream = futures_util::stream::once(async move { Err::<Bytes, _>(e) });
        return Response::new(Body::from_stream(stream));
    }
    let stream = futures_util::stream::unfold((Some(f), count), |(file, left)| async move {
        let f = file.filter(|_| left > 0)?;
        let read = tokio::task::spawn_blocking(move || -> std::io::Result<(std::fs::File, Vec<u8>)> {
            let mut f = f;
            let mut buf = vec![0; CHUNK.min(left as usize)];
            let n = f.read(&mut buf)?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the file shrank while read"));
            }
            buf.truncate(n);
            Ok((f, buf))
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        match read {
            Ok((f, buf)) => {
                let n = buf.len() as u64;
                Some((Ok(Bytes::from(buf)), (Some(f), left - n)))
            }
            // an error ends the body: the client sees a short answer, never a wrong one
            Err(e) => Some((Err(e), (None, 0))),
        }
    });
    let mut res = Response::new(Body::from_stream(stream));
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CONTENT_LENGTH, hv(count.to_string()));
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(header::CONTENT_RANGE, hv(format!("bytes {start}-{}/{len}", start + count - 1)));
    }
    res
}

fn hv(s: String) -> HeaderValue {
    HeaderValue::from_str(&s).expect("ascii header")
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn ranges_are_read_as_rfc_9110_says() {
        assert_eq!(range(None, 10), Ok(None));
        assert_eq!(range(Some("bytes=0-3"), 10), Ok(Some((0, 3))));
        assert_eq!(range(Some("bytes=4-"), 10), Ok(Some((4, 9))));
        assert_eq!(range(Some("bytes=-3"), 10), Ok(Some((7, 9))), "a suffix");
        assert_eq!(range(Some("bytes=-30"), 10), Ok(Some((0, 9))), "a suffix longer than the file");
        assert_eq!(range(Some("bytes=5-99"), 10), Ok(Some((5, 9))), "an end past the file is clipped");
        assert_eq!(range(Some("bytes=0-1,4-5"), 10), Ok(None), "several ranges: the whole file");
        assert_eq!(range(Some("items=0-1"), 10), Ok(None), "another unit: the whole file");
        assert_eq!(range(Some("bytes=10-"), 10), Err(()), "a start past the end");
        assert_eq!(range(Some("bytes=5-2"), 10), Err(()));
        assert_eq!(range(Some("bytes=-0"), 10), Err(()));
        assert_eq!(range(Some("bytes=0-"), 0), Err(()), "an empty file has no byte 0");
        assert_eq!(range(Some("bytes=x-1"), 10), Err(()));
    }

    /// An opened file whose name is already gone: nothing left behind.
    fn file(bytes: &[u8]) -> std::fs::File {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!("tm-raw-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&p, bytes).unwrap();
        let f = std::fs::File::open(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        f
    }

    async fn body(res: Response) -> Vec<u8> {
        res.into_body().collect().await.unwrap().to_bytes().to_vec()
    }

    #[tokio::test]
    async fn a_file_streams_whole_or_in_part() {
        let data: Vec<u8> = (0..(3 * CHUNK + 17)).map(|i| (i % 251) as u8).collect();
        let f = file(&data);
        let res = response(f, data.len() as u64, None);
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(res.headers()[header::CONTENT_LENGTH], data.len().to_string().as_str());
        assert_eq!(body(res).await, data, "several chunks, in order");
        let f = file(&data);
        let res = response(f, data.len() as u64, Some("bytes=65530-65545"));
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()[header::CONTENT_RANGE], format!("bytes 65530-65545/{}", data.len()).as_str());
        assert_eq!(res.headers()[header::CONTENT_LENGTH], "16");
        assert_eq!(body(res).await, data[65530..=65545], "across a chunk boundary");
    }

    #[tokio::test]
    async fn an_unsatisfiable_range_is_416_with_the_length() {
        let f = file(b"0123456789");
        let res = response(f, 10, Some("bytes=10-"));
        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes */10");
        assert!(body(res).await.is_empty());
    }

    #[tokio::test]
    async fn a_file_that_shrinks_while_read_ends_with_an_error() {
        let f = file(&[7; 100]);
        let res = response(f, 200, None);
        assert!(res.into_body().collect().await.is_err(), "a short body, never padded");
    }
}
