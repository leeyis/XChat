//! Desktop Duplication resources stay on the capture thread. A frame is always
//! released and a staging texture always unmapped before another request runs.
use super::{dimensions, packet, rgba, Request, HEADER_LEN};
use crate::remote::{
    model::{Quality, Result, Screen},
    platform,
};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
use windows::{
    core::Interface,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
        System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
            EXECUTION_STATE,
        },
    },
};

#[path = "capture_gpu.rs"]
mod gpu;
#[path = "capture_hevc.rs"]
mod hevc;

const DXGI_FIRST_FRAME_WAITS: u8 = 4;
const DXGI_RETRY_DELAY: Duration = Duration::from_secs(5);

/// A static desktop is healthy only after this duplication delivered a surface.
/// A new/recreated duplication must not return "unchanged" forever at startup.
#[derive(Default)]
struct FirstFrame {
    presented: bool,
    waits: u8,
}

impl FirstFrame {
    fn timed_out(&mut self) -> bool {
        if self.presented {
            return false;
        }
        self.waits = self.waits.saturating_add(1);
        self.waits >= DXGI_FIRST_FRAME_WAITS
    }

    fn received(&mut self) {
        self.presented = true;
        self.waits = 0;
    }
}

#[derive(Default)]
struct GdiFallback {
    active: bool,
    last_pixels: Vec<u8>,
}

impl GdiFallback {
    fn dxgi_received(&mut self) {
        self.active = false;
        // The remote surface may now differ from our last GDI snapshot. Its
        // first frame after another fallback must always be sent again.
        self.last_pixels.clear();
    }

    fn changed(&mut self, pixels: &[u8]) -> bool {
        if pixels == self.last_pixels {
            return false;
        }
        self.last_pixels.clear();
        self.last_pixels.extend_from_slice(pixels);
        true
    }
}

/// Execution state belongs to this OS thread, not to the async request owner.
/// Do not use away mode: explicit sleep and desktop locking remain effective.
struct CaptureAwake {
    previous: EXECUTION_STATE,
    _thread: PhantomData<Rc<()>>,
}

impl CaptureAwake {
    fn new() -> Self {
        let previous = unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)
        };
        if previous.0 == 0 {
            log::warn!("Remote capture could not prevent automatic display/system idle");
        }
        Self {
            previous,
            _thread: PhantomData,
        }
    }
}

impl Drop for CaptureAwake {
    fn drop(&mut self) {
        if self.previous.0 != 0 {
            unsafe { SetThreadExecutionState(self.previous | ES_CONTINUOUS) };
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct FrameTiming {
    acquire_ms: f64,
    prepare_ms: f64,
    readback_ms: f64,
    convert_ms: f64,
    compare_ms: f64,
}

struct Duplication {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    scaler: Option<gpu::Scaler>,
    scale_attempted: bool,
    first_frame: FirstFrame,
    screen: Screen,
    #[cfg(test)]
    timing: FrameTiming,
}

impl Duplication {
    fn new(screen: &Screen) -> Result<Self> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| e.to_string())?;
            let mut adapter_index = 0;
            while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
                adapter_index += 1;
                let mut output_index = 0;
                while let Ok(output) = adapter.EnumOutputs(output_index) {
                    output_index += 1;
                    let desc = output.GetDesc().map_err(|e| e.to_string())?;
                    if (desc.Monitor.0 as u32).to_string() != screen.id {
                        continue;
                    }
                    if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY {
                        return Err("DXGI rotated display requires GDI fallback".into());
                    }
                    let width =
                        (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
                    let height =
                        (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;
                    if !desc.AttachedToDesktop.as_bool()
                        || width != screen.width
                        || height != screen.height
                    {
                        return Err("显示器配置已变化，请重新选择屏幕".into());
                    }
                    let (mut device, mut context) = (None, None);
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                    .map_err(|e| e.to_string())?;
                    let device = device.ok_or("D3D11 device missing")?;
                    let output: IDXGIOutput1 = output.cast().map_err(|e| e.to_string())?;
                    let duplication = output.DuplicateOutput(&device).map_err(|e| e.to_string())?;
                    return Ok(Self {
                        device,
                        context: context.ok_or("D3D11 context missing")?,
                        duplication,
                        staging: None,
                        scaler: None,
                        scale_attempted: false,
                        first_frame: FirstFrame::default(),
                        screen: screen.clone(),
                        #[cfg(test)]
                        timing: FrameTiming::default(),
                    });
                }
            }
        }
        Err("所选显示器已断开，请重新选择屏幕".into())
    }

    fn frame(&mut self, quality: &Quality, wait_ms: u32) -> Result<Option<Vec<u8>>> {
        #[cfg(test)]
        let mut stage = Instant::now();
        unsafe {
            let mut information = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            match self
                .duplication
                .AcquireNextFrame(wait_ms, &mut information, &mut resource)
            {
                Ok(()) => (),
                Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    return self.wait_timeout()
                }
                Err(error) => return Err(error.to_string()),
            }
            // A pointer-only notification has no new desktop surface. Avoid
            // GPU readback and a full-frame CPU comparison for these events.
            if self.first_frame.presented && information.LastPresentTime == 0 {
                self.duplication.ReleaseFrame().map_err(|e| e.to_string())?;
                return Ok(None);
            }
            #[cfg(test)]
            {
                self.timing = FrameTiming::default();
                self.timing.acquire_ms = stage.elapsed().as_secs_f64() * 1000.0;
                stage = Instant::now();
            }
            // This closure ensures ReleaseFrame runs on every conversion/error path.
            let result = (|| {
                let texture: ID3D11Texture2D = resource
                    .ok_or("DXGI frame missing")?
                    .cast()
                    .map_err(|e| e.to_string())?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                texture.GetDesc(&mut desc);
                if desc.Width != self.screen.width
                    || desc.Height != self.screen.height
                    || desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
                {
                    return Err("显示器格式已变化，请重新选择屏幕".into());
                }
                let target = dimensions(&self.screen, quality);
                if !self.scale_attempted {
                    self.scale_attempted = true;
                    match gpu::Scaler::new(&self.device, &self.context, desc, target) {
                        Ok(scaler) => self.scaler = Some(scaler),
                        Err(error) => {
                            log::warn!("Remote GPU scaling unavailable, using CPU: {error}")
                        }
                    }
                }
                let scaled = self
                    .scaler
                    .as_ref()
                    .map(|scaler| scaler.scale(&texture).cloned());
                let mapped_texture = match scaled {
                    Some(Ok(texture)) => texture,
                    Some(Err(error)) => {
                        log::warn!("Remote GPU scale failed, using CPU: {error}");
                        self.scaler = None;
                        self.staging = None;
                        texture.clone()
                    }
                    None => texture.clone(),
                };
                mapped_texture.GetDesc(&mut desc);
                if self.staging.is_none() {
                    desc.Usage = D3D11_USAGE_STAGING;
                    desc.BindFlags = 0;
                    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                    desc.MiscFlags = 0;
                    self.device
                        .CreateTexture2D(&desc, None, Some(&mut self.staging))
                        .map_err(|e| e.to_string())?;
                }
                let staging = self
                    .staging
                    .as_ref()
                    .ok_or("D3D11 staging texture missing")?;
                #[cfg(test)]
                {
                    self.timing.prepare_ms = stage.elapsed().as_secs_f64() * 1000.0;
                    stage = Instant::now();
                }
                self.context.CopyResource(staging, &mapped_texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.context
                    .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(|e| e.to_string())?;
                #[cfg(test)]
                {
                    self.timing.readback_ms = stage.elapsed().as_secs_f64() * 1000.0;
                    stage = Instant::now();
                }
                let result = if mapped.pData.is_null() {
                    Err("D3D11 mapped frame missing".into())
                } else {
                    // DXGI owns exactly RowPitch * Height bytes until Unmap.
                    let source = std::slice::from_raw_parts(
                        mapped.pData.cast(),
                        mapped.RowPitch as usize * desc.Height as usize,
                    );
                    rgba(
                        source,
                        mapped.RowPitch as usize,
                        desc.Width,
                        desc.Height,
                        dimensions(&self.screen, quality),
                        desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
                        quality.reduced_color,
                        HEADER_LEN,
                    )
                    .map(Some)
                };
                self.context.Unmap(staging, 0);
                #[cfg(test)]
                {
                    self.timing.convert_ms = stage.elapsed().as_secs_f64() * 1000.0;
                }
                result
            })();
            let release = self.duplication.ReleaseFrame().map_err(|e| e.to_string());
            release?;
            if result.is_ok() {
                self.first_frame.received();
            }
            result
        }
    }

    fn nv12_frame(&mut self, converter: &gpu::Nv12, wait_ms: u32) -> Result<Option<Vec<u8>>> {
        unsafe {
            let mut information = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            match self
                .duplication
                .AcquireNextFrame(wait_ms, &mut information, &mut resource)
            {
                Ok(()) => (),
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return self.wait_timeout(),
                Err(e) => return Err(e.to_string()),
            }
            if self.first_frame.presented && information.LastPresentTime == 0 {
                self.duplication.ReleaseFrame().map_err(|e| e.to_string())?;
                return Ok(None);
            }
            let result = (|| {
                let texture: ID3D11Texture2D = resource
                    .ok_or("DXGI frame missing")?
                    .cast()
                    .map_err(|e| e.to_string())?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                texture.GetDesc(&mut desc);
                if desc.Width != self.screen.width
                    || desc.Height != self.screen.height
                    || desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
                {
                    return Err("显示器格式已变化，请重新选择屏幕".into());
                }
                converter.readback(&texture).map(Some)
            })();
            let release = self.duplication.ReleaseFrame().map_err(|e| e.to_string());
            release?;
            if result.is_ok() {
                self.first_frame.received();
            }
            result
        }
    }
    fn wait_timeout(&mut self) -> Result<Option<Vec<u8>>> {
        if self.first_frame.timed_out() {
            Err(format!(
                "DXGI did not provide an initial desktop frame after {DXGI_FIRST_FRAME_WAITS} waits"
            ))
        } else {
            Ok(None)
        }
    }
}

struct HevcBackend {
    screen: Screen,
    quality_key: (String, bool, u8),
    coded: (u32, u32),
    dxgi: Duplication,
    nv12: gpu::Nv12,
    encoder: hevc::Encoder,
    last_pixels: Vec<u8>,
    sequence: u64,
    started: Instant,
}

impl HevcBackend {
    fn new(screen: Screen, quality: &Quality) -> Result<Self> {
        let (width, height) = dimensions(&screen, quality);
        // NV12's chroma plane requires even dimensions. Scale down at most one
        // pixel; retain the full original dimensions in XHV1 for input mapping.
        let coded = (width.max(2) & !1, height.max(2) & !1);
        let encoder = hevc::Encoder::new(coded.0, coded.1, quality.fps as u32)?;
        let dxgi =
            Duplication::new(&screen).map_err(|e| format!("remote_hevc_unsupported: DXGI: {e}"))?;
        let source = D3D11_TEXTURE2D_DESC {
            Width: screen.width,
            Height: screen.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            ..Default::default()
        };
        let nv12 = gpu::Nv12::new(&dxgi.device, &dxgi.context, source, coded)
            .map_err(|e| format!("remote_hevc_unsupported: GPU NV12 conversion: {e}"))?;
        Ok(Self {
            screen,
            quality_key: (quality.preset.clone(), quality.reduced_color, quality.fps),
            coded,
            dxgi,
            nv12,
            encoder,
            last_pixels: Vec::new(),
            sequence: 0,
            started: Instant::now(),
        })
    }

    fn matches(&self, request: &Request) -> bool {
        self.screen == request.screen
            && self.quality_key
                == (
                    request.quality.preset.clone(),
                    request.quality.reduced_color,
                    request.quality.fps,
                )
    }

    fn frame(&mut self, request: &Request, wait_ms: u32) -> Result<Vec<u8>> {
        let started = Instant::now();
        let pixels = self
            .dxgi
            .nv12_frame(&self.nv12, wait_ms)
            .map_err(|e| format!("remote_hevc_unsupported: DXGI capture: {e}"))?;
        let changed = pixels.is_some();
        if let Some(pixels) = pixels {
            self.last_pixels = pixels;
        }
        let capture_us = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
        let frame = if !self.last_pixels.is_empty() && (changed || request.request_keyframe) {
            let pts_us = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            let frame = match self
                .encoder
                .encode(&self.last_pixels, request.request_keyframe)
            {
                Ok(frame) => frame,
                // A driver may not implement force-keyframe. A fresh hardware
                // MFT guarantees a new independent stream from cached NV12 even
                // while the desktop is idle. Never fall back to a software MFT.
                Err(_) if request.request_keyframe => {
                    self.encoder =
                        hevc::Encoder::new(self.coded.0, self.coded.1, request.quality.fps as u32)?;
                    self.encoder.encode(&self.last_pixels, false)?
                }
                Err(e) => return Err(e),
            };
            // Sequence belongs to this capture generation, not an encoder
            // instance; forcing a new MFT must not make sequence go backwards.
            self.sequence += 1;
            Some(hevc::Frame {
                sequence: self.sequence,
                pts_us,
                ..frame
            })
        } else {
            None
        };
        Ok(hevc::packet(
            self.encoder.name(),
            self.coded,
            (self.screen.width, self.screen.height),
            request.quality.fps.clamp(1, 60) as u32,
            capture_us,
            request.queued.elapsed().as_micros().min(u32::MAX as u128) as u32,
            self.sequence,
            frame,
        ))
    }
}

struct Backend {
    screen: Screen,
    dxgi: Option<Duplication>,
    monitor: xcap::Monitor,
    retry_at: Instant,
    fallback: GdiFallback,
    quality_key: (String, bool),
    #[cfg(test)]
    timing: FrameTiming,
}

impl Backend {
    fn new(screen: Screen, quality: &Quality) -> Result<Self> {
        let monitor = xcap::Monitor::all()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.id().ok().is_some_and(|id| id.to_string() == screen.id))
            .ok_or("所选显示器已断开，请重新选择屏幕")?;
        let dxgi = match Duplication::new(&screen) {
            Ok(capture) => Some(capture),
            Err(error) => {
                log::warn!("Remote DXGI unavailable, using GDI: {error}");
                None
            }
        };
        let gdi_active = dxgi.is_none();
        Ok(Self {
            screen,
            dxgi,
            monitor,
            retry_at: Instant::now() + DXGI_RETRY_DELAY,
            fallback: GdiFallback {
                active: gdi_active,
                ..Default::default()
            },
            quality_key: (quality.preset.clone(), quality.reduced_color),
            #[cfg(test)]
            timing: FrameTiming::default(),
        })
    }

    fn frame(&mut self, quality: &Quality, wait_ms: u32) -> Result<(u8, Option<Vec<u8>>)> {
        let key = (quality.preset.clone(), quality.reduced_color);
        if key != self.quality_key {
            self.fallback.last_pixels.clear();
            // Release the old duplication before creating its replacement. A
            // fresh instance has its own bounded wait for the first surface.
            self.dxgi = None;
            self.dxgi = Duplication::new(&self.screen).ok();
            self.retry_at = Instant::now() + DXGI_RETRY_DELAY;
            self.fallback.active |= self.dxgi.is_none();
            self.quality_key = key;
        }
        if self.dxgi.is_none() && Instant::now() >= self.retry_at {
            self.dxgi = Duplication::new(&self.screen).ok();
            self.retry_at = Instant::now() + DXGI_RETRY_DELAY;
        }
        if let Some(dxgi) = self.dxgi.as_mut() {
            match dxgi.frame(quality, wait_ms) {
                // Keep delivering real GDI frames during a retry probe until
                // DXGI actually supplies a surface, with the GDI backend tag.
                Ok(None) if self.fallback.active => (),
                Ok(None) => return Ok((1, None)),
                Ok(Some(pixels)) => {
                    #[cfg(test)]
                    {
                        self.timing = dxgi.timing;
                    }
                    self.fallback.dxgi_received();
                    return Ok((1, Some(pixels)));
                }
                Err(error) => {
                    log::warn!("Remote DXGI unavailable, using GDI before retry: {error}");
                    self.dxgi = None;
                    self.retry_at = Instant::now() + DXGI_RETRY_DELAY;
                    self.fallback.active = true;
                }
            }
        }
        let image = self.monitor.capture_image().map_err(|e| e.to_string())?;
        if (image.width(), image.height()) != (self.screen.width, self.screen.height) {
            return Err("显示器配置已变化，请重新选择屏幕".into());
        }
        let pixels = rgba(
            image.as_raw(),
            image.width() as usize * 4,
            image.width(),
            image.height(),
            dimensions(&self.screen, quality),
            false,
            quality.reduced_color,
            HEADER_LEN,
        )?;
        // DXGI reports desktop changes; only GDI needs this fallback comparison.
        #[cfg(test)]
        let compare = Instant::now();
        if !self.fallback.changed(&pixels) {
            return Ok((2, None));
        }
        #[cfg(test)]
        {
            self.timing.compare_ms = compare.elapsed().as_secs_f64() * 1000.0;
        }
        Ok((2, Some(pixels)))
    }
}

pub(super) fn run(requests: Receiver<Request>) {
    let mut backend: Option<Backend> = None;
    let mut hevc_backend: Option<HevcBackend> = None;
    let mut generation = String::new();
    let mut sequence = 0;
    let mut next_frame = Instant::now();
    let mut awake = None;
    // No requests means no capture; idle timeout also releases the execution
    // state request and GPU/desktop handles on their owning OS thread.
    while let Ok(request) = requests.recv_timeout(Duration::from_secs(2)) {
        if request.reply.is_closed() {
            continue;
        }
        // Browser timers are clamped for occluded/minimized WebViews. Pace the
        // native requests here so continued IPC callbacks need no JS timer.
        let period = Duration::from_secs_f64(1.0 / request.quality.fps.clamp(1, 60) as f64);
        if request.generation == generation {
            if let Some(wait) = next_frame.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait.min(period));
            }
        }
        if request.reply.is_closed() {
            continue;
        }
        let started = Instant::now();
        next_frame = started + period;
        let result = (|| {
            if !platform::interactive() {
                return Err("等待桌面解锁后恢复共享".into());
            }
            awake.get_or_insert_with(CaptureAwake::new);
            if request.hevc {
                // Format switches and owner/revision changes release the old
                // pipeline before accepting any new frame. No stale pixel or
                // encoder state crosses the caller's generation token.
                backend = None;
                if generation != request.generation
                    || hevc_backend.as_ref().is_none_or(|b| !b.matches(&request))
                {
                    hevc_backend = None;
                    hevc_backend =
                        Some(HevcBackend::new(request.screen.clone(), &request.quality)?);
                    generation = request.generation.clone();
                }
                let packet = hevc_backend
                    .as_mut()
                    .unwrap()
                    .frame(&request, period.as_millis().clamp(1, 100) as u32)?;
                if !platform::interactive() {
                    hevc_backend = None;
                    return Err("等待桌面解锁后恢复共享".into());
                }
                return Ok(packet);
            }
            hevc_backend = None;
            if generation != request.generation
                || backend.as_ref().is_none_or(|b| b.screen != request.screen)
            {
                backend = None;
                backend = Some(Backend::new(request.screen.clone(), &request.quality)?);
                generation = request.generation.clone();
            }
            let (method, pixels) = backend
                .as_mut()
                .unwrap()
                .frame(&request.quality, period.as_millis().clamp(1, 100) as u32)?;
            if !platform::interactive() {
                backend = None;
                return Err("等待桌面解锁后恢复共享".into());
            }
            if pixels.is_some() {
                sequence += 1;
            }
            Ok(packet(
                &request.screen,
                &request.quality,
                sequence,
                method,
                started.elapsed().as_micros().min(u32::MAX as u128) as u32,
                request.queued.elapsed().as_micros().min(u32::MAX as u128) as u32,
                pixels,
            ))
        })();
        if result.is_err() {
            backend = None;
            hevc_backend = None;
            awake = None;
        }
        let _ = request.reply.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_surface_wait_has_a_finite_budget() {
        let mut first = FirstFrame::default();
        for _ in 1..DXGI_FIRST_FRAME_WAITS {
            assert!(!first.timed_out());
        }
        assert!(first.timed_out());
        // Saturation must not accidentally start another waiting period.
        for _ in 0..300 {
            assert!(first.timed_out());
        }
    }

    #[test]
    fn an_existing_surface_remains_valid_on_a_static_desktop() {
        let mut first = FirstFrame::default();
        first.received();
        for _ in 0..1000 {
            assert!(!first.timed_out());
        }
    }

    #[test]
    fn a_late_first_surface_cancels_the_startup_timeout() {
        let mut first = FirstFrame::default();
        for _ in 1..DXGI_FIRST_FRAME_WAITS {
            assert!(!first.timed_out());
        }
        first.received();
        for _ in 0..1000 {
            assert!(!first.timed_out());
        }
    }

    #[test]
    fn recreated_duplication_gets_its_own_first_surface_budget() {
        let mut previous = FirstFrame::default();
        previous.received();
        assert!(!previous.timed_out());
        // Both quality/revision changes and a GDI-to-DXGI retry create a new
        // duplication. Pixels delivered by its predecessor cannot satisfy it.
        let mut retry = FirstFrame::default();
        for _ in 1..DXGI_FIRST_FRAME_WAITS {
            assert!(!retry.timed_out());
        }
        assert!(retry.timed_out());
        let mut recovered = FirstFrame::default();
        assert!(!recovered.timed_out());
        recovered.received();
        assert!(!recovered.timed_out());
    }

    #[test]
    fn a_gdi_dxgi_gdi_transition_resends_the_first_gdi_frame() {
        let mut fallback = GdiFallback {
            active: true,
            ..Default::default()
        };
        let pixels = [0, 1, 2, 255];
        assert!(fallback.changed(&pixels));
        assert!(!fallback.changed(&pixels));
        // A DXGI retry with no surface does not call dxgi_received; real GDI
        // remains active and its unchanged frames still use the GDI tag.
        let mut retry = FirstFrame::default();
        assert!(!retry.timed_out());
        assert!(fallback.active);
        assert!(!fallback.changed(&pixels));
        retry.received();
        fallback.dxgi_received();
        assert!(!fallback.active);
        fallback.active = true;
        assert!(fallback.changed(&pixels));
        assert!(!fallback.changed(&pixels));
    }
}

#[cfg(test)]
mod benchmark {
    use super::*;
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            eprintln!("{}: {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
    #[test]
    #[ignore = "captures the local desktop for timing; run manually on an interactive Windows desktop"]
    fn compare_capture_paths() {
        static LOGGER: Logger = Logger;
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Warn);
        let screen = platform::screens().unwrap().into_iter().next().unwrap();
        let quality = Quality {
            preset: "clear".into(),
            fps: 30,
            reduced_color: false,
        };
        let report = |name: &str, mut times: Vec<f64>, bytes: usize| {
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            println!("{name}: source={}x{}, output={:?}, samples={}, mean_ms={mean:.2}, p95_ms={:.2}, last_bytes={bytes}",
                screen.width, screen.height, dimensions(&screen, &quality), times.len(), times[times.len()*95/100]);
        };
        let mut legacy = Vec::new();
        let mut bytes = 0;
        let samples = std::env::var("XCHAT_CAPTURE_BENCH_LEGACY")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(30);
        for _ in 0..samples {
            let start = Instant::now();
            bytes = platform::frame(&screen, &quality).unwrap().len();
            legacy.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        if !legacy.is_empty() {
            report("legacy-GDI-JPEG", legacy, bytes);
        }
        let mut capture = Backend::new(screen.clone(), &quality).unwrap();
        println!(
            "new capture backend={}",
            if capture.dxgi.is_some() {
                "DXGI"
            } else {
                "GDI"
            }
        );
        let mut times = Vec::new();
        let mut phases = Vec::new();
        let mut unchanged = 0;
        let capture_started = Instant::now();
        let deadline = capture_started + Duration::from_secs(8);
        while times.len() < 240 && Instant::now() < deadline {
            let start = Instant::now();
            let (_, pixels) = capture.frame(&quality, 17).unwrap();
            if let Some(pixels) = pixels {
                bytes = pixels.len() - HEADER_LEN;
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                phases.push(capture.timing);
            } else {
                unchanged += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if !times.is_empty() {
            println!(
                "capture_changed_fps={:.2}",
                times.len() as f64 / capture_started.elapsed().as_secs_f64()
            );
            report("persistent-raw", times, bytes);
            let average = |field: fn(&FrameTiming) -> f64| {
                phases.iter().map(field).sum::<f64>() / phases.len() as f64
            };
            println!("phases mean_ms: acquire={:.2}, prepare_gpu={:.2}, readback={:.2}, convert={:.2}, compare_copy={:.2}",
                average(|t| t.acquire_ms), average(|t| t.prepare_ms), average(|t| t.readback_ms),
                average(|t| t.convert_ms), average(|t| t.compare_ms));
        }
        println!(
            "gpu_scaling={}",
            capture.dxgi.as_ref().is_some_and(|d| d.scaler.is_some())
        );
        println!("unchanged requests excluded from frame count: {unchanged}");
    }
}
