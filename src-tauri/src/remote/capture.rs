//! Demand-driven native video capture. The GPU context belongs to one worker,
//! never to Tokio's pool; a slow renderer cannot accumulate queued screenshots.
use super::model::{Quality, Result, Screen};

#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
pub const HEADER_LEN: usize = 44;

#[derive(Default)]
pub struct Capture {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    worker: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<Request>>>,
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
struct Request {
    queued: std::time::Instant,
    generation: String,
    screen: Screen,
    quality: Quality,
    hevc: bool,
    request_keyframe: bool,
    reply: tokio::sync::oneshot::Sender<Result<Vec<u8>>>,
}

impl Capture {
    pub async fn frame(
        &self,
        generation: String,
        screen: Screen,
        quality: Quality,
    ) -> Result<Vec<u8>> {
        self.request(generation, screen, quality, false, false)
            .await
    }

    /// A hardware-only Annex B HEVC access unit with an XHV1 metadata header.
    /// Unsupported encoders return an explicit error for transport negotiation.
    pub async fn hevc_frame(
        &self,
        generation: String,
        screen: Screen,
        quality: Quality,
        request_keyframe: bool,
    ) -> Result<Vec<u8>> {
        self.request(generation, screen, quality, true, request_keyframe)
            .await
    }

    async fn request(
        &self,
        generation: String,
        screen: Screen,
        quality: Quality,
        hevc: bool,
        request_keyframe: bool,
    ) -> Result<Vec<u8>> {
        #[cfg(all(feature = "desktop", target_os = "windows"))]
        {
            // A request racing the idle worker's shutdown may be accepted by
            // its channel just before the receiver drops. Retry that race once.
            for attempt in 0..2 {
                let (reply, receiver) = tokio::sync::oneshot::channel();
                let mut request = Some(Request {
                    queued: std::time::Instant::now(),
                    generation: generation.clone(),
                    screen: screen.clone(),
                    quality: quality.clone(),
                    hevc,
                    request_keyframe,
                    reply,
                });
                {
                    let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(sender) = worker.as_ref() {
                        match sender.try_send(request.take().unwrap()) {
                            Ok(()) => (),
                            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                                return Err("remote_capture_busy".into())
                            }
                            Err(std::sync::mpsc::TrySendError::Disconnected(value)) => {
                                request = Some(value)
                            }
                        }
                    }
                    if let Some(request) = request {
                        let (sender, requests) = std::sync::mpsc::sync_channel(1);
                        sender.try_send(request).map_err(|e| e.to_string())?;
                        std::thread::Builder::new()
                            .name("remote-capture".into())
                            .spawn(move || windows_capture::run(requests))
                            .map_err(|e| e.to_string())?;
                        *worker = Some(sender);
                    }
                }
                match receiver.await {
                    Ok(result) => return result,
                    Err(_) if attempt == 0 => {
                        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    }
                    Err(_) => return Err("屏幕采集线程已退出".into()),
                }
            }
            unreachable!();
        }
        #[cfg(not(all(feature = "desktop", target_os = "windows")))]
        {
            let _ = (generation, screen, quality, request_keyframe);
            Err(if hevc {
                "remote_hevc_unsupported: 当前平台不支持原生 HEVC 编码"
            } else {
                "当前平台不支持原生屏幕采集"
            }
            .into())
        }
    }
}

#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
fn dimensions(screen: &Screen, quality: &Quality) -> (u32, u32) {
    let maximum = match quality.preset.as_str() {
        "fluent" => 960,
        "clear" => 1920,
        _ => 1440,
    };
    let largest = screen.width.max(screen.height);
    if largest <= maximum {
        return (screen.width, screen.height);
    }
    (
        (screen.width as u64 * maximum as u64 / largest as u64).max(1) as u32,
        (screen.height as u64 * maximum as u64 / largest as u64).max(1) as u32,
    )
}

#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
fn packet(
    screen: &Screen,
    quality: &Quality,
    sequence: u64,
    backend: u8,
    capture_us: u32,
    request_us: u32,
    pixels: Option<Vec<u8>>,
) -> Vec<u8> {
    let (width, height) = dimensions(screen, quality);
    let unchanged = pixels.is_none();
    // Capture reserves this prefix up front: adding the header must not copy
    // another full-resolution frame before moving it into the IPC response.
    let mut bytes = pixels.unwrap_or_else(|| vec![0; HEADER_LEN]);
    bytes[..4].copy_from_slice(b"XRF1");
    bytes[4..6].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
    bytes[6..8].copy_from_slice(&[1, backend]);
    bytes[8..12].copy_from_slice(&width.to_le_bytes());
    bytes[12..16].copy_from_slice(&height.to_le_bytes());
    bytes[16..24].copy_from_slice(&sequence.to_le_bytes());
    bytes[24..28].copy_from_slice(&capture_us.to_le_bytes());
    bytes[28..32].copy_from_slice(&screen.width.to_le_bytes());
    bytes[32..36].copy_from_slice(&screen.height.to_le_bytes());
    bytes[36..40].copy_from_slice(&u32::from(unchanged).to_le_bytes());
    bytes[40..44].copy_from_slice(&request_us.to_le_bytes());
    bytes
}

/// Fused BGRA conversion and bilinear scaling, without JPEG or intermediate
/// full-size RGB allocations. Source stride comes from D3D, not width * 4.
#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
fn rgba(
    source: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    target: (u32, u32),
    bgra: bool,
    reduced: bool,
    prefix: usize,
) -> Result<Vec<u8>> {
    let (tw, th) = target;
    let row = (width as usize)
        .checked_mul(4)
        .ok_or("invalid capture dimensions")?;
    let length = stride
        .checked_mul(height as usize)
        .ok_or("invalid capture stride")?;
    if width == 0 || height == 0 || tw == 0 || th == 0 || stride < row || source.len() < length {
        return Err("invalid capture buffer".into());
    }
    let mut bytes = vec![0; prefix + tw as usize * th as usize * 4];
    let output = &mut bytes[prefix..];
    let channels = if bgra { [2, 1, 0] } else { [0, 1, 2] };
    let mask = if reduced { 0xf8 } else { 0xff };
    if (width, height) == target {
        if !bgra && !reduced {
            // The GPU output explicitly fills alpha with 1.0. A tight native
            // copy avoids another per-pixel channel conversion on the CPU.
            for (src, dst) in source
                .chunks_exact(stride)
                .zip(output.chunks_exact_mut(row))
            {
                dst.copy_from_slice(&src[..row]);
            }
            return Ok(bytes);
        }
        for (src, dst) in source
            .chunks_exact(stride)
            .zip(output.chunks_exact_mut(row))
        {
            copy_rgba_row(&src[..row], dst, bgra, reduced);
        }
        return Ok(bytes);
    }
    let xs: Vec<_> = (0..tw)
        .map(|x| {
            let position = x as u64 * (width - 1) as u64 * 256 / tw.saturating_sub(1).max(1) as u64;
            ((position / 256) as usize, (position % 256) as u32)
        })
        .collect();
    for y in 0..th {
        let position = y as u64 * (height - 1) as u64 * 256 / th.saturating_sub(1).max(1) as u64;
        let sy = (position / 256) as usize;
        let fy = (position % 256) as u32;
        let top = &source[sy * stride..];
        let bottom = &source[(sy + 1).min(height as usize - 1) * stride..];
        for (x, &(sx, fx)) in xs.iter().enumerate() {
            let right = (sx + 1).min(width as usize - 1);
            let dst = &mut output[(y as usize * tw as usize + x) * 4..][..4];
            for channel in 0..3 {
                let c = channels[channel];
                let a = top[sx * 4 + c] as u32 * (256 - fx) + top[right * 4 + c] as u32 * fx;
                let b = bottom[sx * 4 + c] as u32 * (256 - fx) + bottom[right * 4 + c] as u32 * fx;
                dst[channel] = (((a * (256 - fy) + b * fy + 32768) >> 16) as u8) & mask;
            }
            dst[3] = 255;
        }
    }
    Ok(bytes)
}

#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
fn copy_rgba_row(source: &[u8], output: &mut [u8], bgra: bool, reduced: bool) {
    let mut offset = 0;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // SSE2 is an x86-64 baseline feature. Each unaligned load/store stays
        // inside complete 16-byte slices; partial tails use the scalar path.
        use std::arch::x86_64::*;
        let low = _mm_set1_epi32(0xff);
        let green = _mm_set1_epi32(0xff00);
        let alpha = _mm_set1_epi32(0xff000000_u32 as i32);
        let mask = _mm_set1_epi32(if reduced { 0x00f8f8f8 } else { 0x00ffffff });
        while offset + 16 <= output.len() {
            let mut pixels = _mm_loadu_si128(source.as_ptr().add(offset).cast());
            if bgra {
                let red = _mm_and_si128(_mm_srli_epi32::<16>(pixels), low);
                let blue = _mm_slli_epi32::<16>(_mm_and_si128(pixels, low));
                pixels = _mm_or_si128(_mm_or_si128(red, blue), _mm_and_si128(pixels, green));
            }
            _mm_storeu_si128(
                output.as_mut_ptr().add(offset).cast(),
                _mm_or_si128(_mm_and_si128(pixels, mask), alpha),
            );
            offset += 16;
        }
    }
    let channels = if bgra { [2, 1, 0] } else { [0, 1, 2] };
    let mask = if reduced { 0xf8 } else { 0xff };
    for (src, dst) in source[offset..]
        .chunks_exact(4)
        .zip(output[offset..].chunks_exact_mut(4))
    {
        for channel in 0..3 {
            dst[channel] = src[channels[channel]] & mask;
        }
        dst[3] = 255;
    }
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
#[path = "capture_windows.rs"]
mod windows_capture;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_frames_honor_stride_color_order_and_scaling_without_encoding() {
        let source = [
            3, 2, 1, 0, 6, 5, 4, 0, 99, 99, 99, 99, 9, 8, 7, 0, 12, 11, 10, 0, 99, 99, 99, 99,
        ];
        assert_eq!(
            rgba(&source, 12, 2, 2, (2, 2), true, false, 0).unwrap(),
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        let scaled = rgba(&source, 12, 2, 2, (3, 3), true, false, 0).unwrap();
        assert_eq!(&scaled[16..20], &[6, 7, 8, 255]);
        assert!(rgba(&source[..10], 12, 2, 2, (2, 2), true, false, 0).is_err());
        // Five pixels cover both the 16-byte SIMD block and the scalar tail.
        let source: Vec<u8> = (0..20).collect();
        assert_eq!(
            rgba(&source, 20, 5, 1, (5, 1), true, true, 0).unwrap(),
            [0, 0, 0, 255, 0, 0, 0, 255, 8, 8, 8, 255, 8, 8, 8, 255, 16, 16, 16, 255]
        );
        let screen = Screen {
            id: "1".into(),
            name: "screen".into(),
            width: 2,
            height: 2,
        };
        let bytes = packet(&screen, &Quality::default(), 9, 1, 1234, 5000, None);
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(&bytes[..4], b"XRF1");
        assert_eq!(u32::from_le_bytes(bytes[36..40].try_into().unwrap()), 1);
        let pixels = rgba(&source[..16], 8, 2, 2, (2, 2), true, false, HEADER_LEN).unwrap();
        let allocation = pixels.as_ptr();
        let bytes = packet(
            &screen,
            &Quality::default(),
            10,
            1,
            1234,
            5000,
            Some(pixels),
        );
        assert_eq!(
            bytes.as_ptr(),
            allocation,
            "packet must reuse the pixel allocation"
        );
        assert_eq!(bytes.len(), HEADER_LEN + 16);
        assert_eq!(&bytes[HEADER_LEN..HEADER_LEN + 4], &[2, 1, 0, 255]);
        assert_eq!(u32::from_le_bytes(bytes[36..40].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 5000);
    }
}
