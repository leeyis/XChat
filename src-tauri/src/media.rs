use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Response, StatusCode},
};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Serialize)]
pub struct MediaSource {
    pub url: String,
    pub mime_type: String,
    pub file_name: String,
    pub file_size: u64,
}

pub fn mime_type(file_name: &str) -> String {
    // Keep the container's MIME accurate even when the platform MIME database
    // associates MP4 with video or returns an obsolete WAV type.
    match std::path::Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "m4a" => "audio/mp4".to_string(),
        "aac" => "audio/aac".to_string(),
        "wav" => "audio/wav".to_string(),
        _ => mime_guess::from_path(file_name)
            .first_or_octet_stream()
            .to_string(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    length: u64,
    partial: bool,
}

// A single byte range is enough for native browser media seeking. Reject
// multiple ranges rather than ever returning the wrong subset of a file.
fn byte_range(value: Option<&str>, size: u64) -> Result<ByteRange, ()> {
    let Some(value) = value else {
        return Ok(ByteRange {
            start: 0,
            length: size,
            partial: false,
        });
    };
    let value = value.trim().strip_prefix("bytes=").ok_or(())?;
    let (first, last) = value.split_once('-').ok_or(())?;
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(());
        }
        value.parse::<u64>().map_err(|_| ())
    };
    if size == 0 {
        return Err(());
    }
    let (start, end) = if first.is_empty() {
        let suffix = number(last)?;
        if suffix == 0 {
            return Err(());
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        let start = number(first)?;
        let end = if last.is_empty() {
            size - 1
        } else {
            number(last)?.min(size - 1)
        };
        if start >= size || end < start {
            return Err(());
        }
        (start, end)
    };
    Ok(ByteRange {
        start,
        length: end - start + 1,
        partial: true,
    })
}

pub struct MediaFile {
    pub file: tokio::fs::File,
    pub name: String,
    pub size: u64,
    // Android dup() shares the original file's seek cursor. Use positional
    // reads for provider descriptors so playback cannot move an upload cursor.
    #[cfg(target_os = "android")]
    pub positional: bool,
}

impl MediaFile {
    pub fn ordinary(file: tokio::fs::File, name: String, size: u64) -> Self {
        Self {
            file,
            name,
            size,
            #[cfg(target_os = "android")]
            positional: false,
        }
    }

    pub fn source(&self, url: String) -> MediaSource {
        MediaSource {
            url,
            mime_type: mime_type(&self.name),
            file_name: self.name.clone(),
            file_size: self.size,
        }
    }
}

pub async fn response(
    mut media: MediaFile,
    method: &Method,
    headers: &HeaderMap,
) -> axum::response::Response {
    // HTTP HEAD describes the full representation and must ignore Range.
    let requested = if method == Method::HEAD || headers.contains_key(header::IF_RANGE) {
        // No validators are emitted, so an If-Range condition cannot be proven
        // to match. Return the complete representation as HTTP requires.
        None
    } else if headers.get_all(header::RANGE).iter().count() > 1 {
        Some("invalid")
    } else {
        match headers.get(header::RANGE) {
            Some(value) => match value.to_str() {
                Ok(value) => Some(value),
                Err(_) => Some("invalid"),
            },
            None => None,
        }
    };
    let range = match byte_range(requested, media.size) {
        Ok(range) => range,
        Err(()) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_TYPE, mime_type(&media.name))
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes */{}", media.size))
                .header(header::CONTENT_LENGTH, 0)
                .header(header::CACHE_CONTROL, "private, no-store")
                .header("x-content-type-options", "nosniff")
                .body(Body::empty())
                .unwrap();
        }
    };
    let mut builder = Response::builder()
        .status(if range.partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, mime_type(&media.name))
        .header(header::CONTENT_LENGTH, range.length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header("x-content-type-options", "nosniff");
    if range.partial {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{}",
                range.start,
                range.start + range.length - 1,
                media.size
            ),
        );
    }
    if method == Method::HEAD || range.length == 0 {
        return builder.body(Body::empty()).unwrap();
    }
    #[cfg(target_os = "android")]
    if media.positional {
        use std::os::unix::fs::FileExt;
        let file = std::sync::Arc::new(media.file.into_std().await);
        let stream = futures_util::stream::try_unfold(
            (file, range.start, range.length),
            |(file, position, remaining)| async move {
                if remaining == 0 {
                    return Ok::<_, std::io::Error>(None);
                }
                let reader = file.clone();
                let length = remaining.min(64 * 1024) as usize;
                let bytes = tokio::task::spawn_blocking(move || {
                    let mut bytes = vec![0; length];
                    let read = reader.read_at(&mut bytes, position)?;
                    if read == 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "media provider ended early",
                        ));
                    }
                    bytes.truncate(read);
                    Ok::<_, std::io::Error>(bytes)
                })
                .await
                .map_err(std::io::Error::other)??;
                let read = bytes.len() as u64;
                Ok(Some((bytes, (file, position + read, remaining - read))))
            },
        );
        return builder.body(Body::from_stream(stream)).unwrap();
    }
    if media
        .file
        .seek(std::io::SeekFrom::Start(range.start))
        .await
        .is_err()
    {
        return Response::builder()
            .status(StatusCode::UNPROCESSABLE_ENTITY)
            .body(Body::from("媒体文件无法定位，请下载后打开"))
            .unwrap();
    }
    builder
        .body(Body::from_stream(
            tokio_util::io::ReaderStream::with_capacity(media.file.take(range.length), 64 * 1024),
        ))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_single_ranges_cover_bounded_open_suffix_and_invalid_requests() {
        for (value, start, length) in [
            ("bytes=2-5", 2, 4),
            ("bytes=7-", 7, 3),
            ("bytes=-4", 6, 4),
            ("bytes=-100", 0, 10),
            ("bytes=2-100", 2, 8),
        ] {
            assert_eq!(
                byte_range(Some(value), 10),
                Ok(ByteRange {
                    start,
                    length,
                    partial: true
                })
            );
        }
        for value in [
            "bytes=10-",
            "bytes=5-2",
            "bytes=-0",
            "bytes=0-1,4-5",
            "bytes=",
            "bytes=+1-2",
            "items=0-1",
        ] {
            assert_eq!(byte_range(Some(value), 10), Err(()), "{value}");
        }
        assert_eq!(byte_range(Some("bytes=0-"), 0), Err(()));
        assert_eq!(
            byte_range(None, 0),
            Ok(ByteRange {
                start: 0,
                length: 0,
                partial: false
            })
        );
    }

    #[tokio::test]
    async fn media_response_streams_exact_ranges_and_head_has_no_body() {
        let path = std::env::temp_dir().join(format!("xchat-range-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, b"0123456789").await.unwrap();
        for (method, requested, status, expected, range, length) in [
            (Method::GET, None, StatusCode::OK, "0123456789", None, "10"),
            (
                Method::GET,
                Some("bytes=2-5"),
                StatusCode::PARTIAL_CONTENT,
                "2345",
                Some("bytes 2-5/10"),
                "4",
            ),
            (
                Method::GET,
                Some("bytes=-3"),
                StatusCode::PARTIAL_CONTENT,
                "789",
                Some("bytes 7-9/10"),
                "3",
            ),
            (
                Method::GET,
                Some("bytes=99-"),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "",
                Some("bytes */10"),
                "0",
            ),
            (
                Method::HEAD,
                Some("bytes=2-5"),
                StatusCode::OK,
                "",
                None,
                "10",
            ),
        ] {
            let mut headers = HeaderMap::new();
            if let Some(value) = requested {
                headers.insert(header::RANGE, value.parse().unwrap());
            }
            let media = MediaFile::ordinary(
                tokio::fs::File::open(&path).await.unwrap(),
                "clip.mp4".into(),
                10,
            );
            let response = response(media, &method, &headers).await;
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], length);
            assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .map(|value| value.to_str().unwrap()),
                range
            );
            if status != StatusCode::RANGE_NOT_SATISFIABLE {
                assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
            }
            let body = axum::body::to_bytes(response.into_body(), 100)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), expected.as_bytes());
        }
        let mut headers = HeaderMap::new();
        headers.append(header::RANGE, "bytes=0-1".parse().unwrap());
        headers.append(header::RANGE, "bytes=4-5".parse().unwrap());
        let media = MediaFile::ordinary(
            tokio::fs::File::open(&path).await.unwrap(),
            "clip.mp4".into(),
            10,
        );
        assert_eq!(
            response(media, &Method::GET, &headers).await.status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        headers.insert(header::IF_RANGE, "\"stale-validator\"".parse().unwrap());
        let media = MediaFile::ordinary(
            tokio::fs::File::open(&path).await.unwrap(),
            "clip.mp4".into(),
            10,
        );
        let complete = response(media, &Method::GET, &headers).await;
        assert_eq!(complete.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(complete.into_body(), 100)
                .await
                .unwrap()
                .as_ref(),
            b"0123456789"
        );
        tokio::fs::remove_file(path).await.unwrap();
    }
}
