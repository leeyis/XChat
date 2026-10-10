//! Hardware-only, demand-driven HEVC encoding. The owning capture worker is the
//! only thread that calls the encoder. MF callbacks only deliver async results.
//! NV12 crosses a staging readback into an MF buffer; this is not zero-copy.
use std::{
    mem::ManuallyDrop,
    ptr,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::{implement, Error, Interface, Ref, GUID},
    Win32::{
        Foundation::E_NOTIMPL,
        Media::MediaFoundation::*,
        System::{Com::*, Variant::VARIANT},
    },
};

type Result<T> = std::result::Result<T, String>;
pub(super) const HEADER_LEN: usize = 68;
const MAX_ACCESS_UNIT: usize = 8 * 1024 * 1024;

fn error(stage: &str, error: Error) -> String {
    format!(
        "remote_hevc_encode: {stage}: 0x{:08X} {error}",
        error.code().0 as u32
    )
}

trait At<T> {
    fn at(self, stage: &str) -> Result<T>;
}
impl<T> At<T> for windows::core::Result<T> {
    fn at(self, stage: &str) -> Result<T> {
        self.map_err(|e| error(stage, e))
    }
}

struct Runtime;
impl Runtime {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .at("CoInitializeEx")?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL).at("MFStartup") {
                CoUninitialize();
                return Err(error);
            }
        }
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

// MF explicitly permits completing a Begin/End operation on another thread.
// Only the async result crosses from MF's MTA callback to our MTA capture worker;
// the encoder interfaces and EndGetEvent remain on the capture worker.
// https://learn.microsoft.com/windows/win32/medfound/calling-asynchronous-methods
struct EventCompletion(IMFAsyncResult);
unsafe impl Send for EventCompletion {}

#[derive(Default)]
struct EventSlotState {
    completion: Option<EventCompletion>,
    closed: bool,
    invalid: bool,
}

#[derive(Default)]
struct EventSlot {
    state: Mutex<EventSlotState>,
    ready: Condvar,
}

impl EventSlot {
    fn deliver(&self, mut completion: Option<EventCompletion>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.closed {
            if state.completion.is_none() && completion.is_some() {
                state.completion = completion.take();
            } else {
                // One outstanding BeginGetEvent permits exactly one result.
                // Reject malformed/duplicate callbacks without growing a queue.
                state.invalid = true;
            }
        }
        drop(state);
        // Release COM references outside the mutex, including late completions.
        drop(completion);
        self.ready.notify_one();
    }

    fn wait(&self, deadline: Instant) -> Result<Option<EventCompletion>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.closed {
                return Err("remote_hevc_encode: event wait closed".into());
            }
            if state.invalid {
                return Err("remote_hevc_encode: invalid event callback".into());
            }
            if let Some(completion) = state.completion.take() {
                return Ok(Some(completion));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            // The absolute deadline survives spurious wakeups. There is no
            // polling or timer-resolution dependency on the successful path.
            state = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn close(&self) {
        let completion = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state.completion.take()
        };
        self.ready.notify_all();
        drop(completion);
    }
}

#[implement(IMFAsyncCallback)]
struct EventCallback {
    slot: Arc<EventSlot>,
}

impl IMFAsyncCallback_Impl for EventCallback_Impl {
    fn GetParameters(&self, _: *mut u32, _: *mut u32) -> windows::core::Result<()> {
        // Let MF select its normal callback work queue.
        Err(E_NOTIMPL.into())
    }

    fn Invoke(&self, result: Ref<IMFAsyncResult>) -> windows::core::Result<()> {
        self.slot.deliver(result.cloned().map(EventCompletion));
        Ok(())
    }
}

struct EventWait {
    slot: Arc<EventSlot>,
    callback: IMFAsyncCallback,
    pending: bool,
}

impl EventWait {
    fn new() -> Self {
        let slot = Arc::new(EventSlot::default());
        let callback = EventCallback { slot: slot.clone() }.into();
        Self {
            slot,
            callback,
            pending: false,
        }
    }

    fn wait(
        &mut self,
        deadline: Instant,
        begin: impl FnOnce(&IMFAsyncCallback) -> windows::core::Result<()>,
    ) -> Result<Option<EventCompletion>> {
        // Never hold the slot mutex while entering MF. Even an immediate
        // callback can publish its result without waiting on a worker COM call.
        if !self.pending {
            begin(&self.callback).at("BeginGetEvent")?;
            self.pending = true;
        }
        let completion = self.slot.wait(deadline)?;
        if completion.is_some() {
            self.pending = false;
        }
        // After a timeout the same request remains pending, so a retry cannot
        // accidentally add a second subscriber or consume an unrelated event.
        Ok(completion)
    }
}

impl Drop for EventWait {
    fn drop(&mut self) {
        self.slot.close();
    }
}

struct Transform {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    event_wait: EventWait,
    codec: ICodecAPI,
    activation: IMFActivate,
}
impl Drop for Transform {
    fn drop(&mut self) {
        // Close before FLUSH/ShutdownObject, which may dispatch a last callback.
        // The callback owns only the slot, never the generator/transform, and
        // never re-arms itself. A late callback therefore cannot resurrect MF
        // work, create a reference cycle, or access the dropped capture worker.
        self.event_wait.slot.close();
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = self.activation.ShutdownObject();
        }
    }
}

/// One compressed, independently framed Annex B access unit.
pub(super) struct Frame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub sequence: u64,
    pub pts_us: u64,
    pub encode_us: u32,
}

pub(super) struct Encoder {
    mft: Transform,
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    input_id: u32,
    output_id: u32,
    input_ready: u32,
    sequence: u64,
    start: Instant,
    parameter_sets: [Vec<u8>; 3],
    // Dropped after every COM interface above, on the same capture thread.
    _runtime: Runtime,
}

impl Encoder {
    pub(super) fn new(width: u32, height: u32, fps: u32) -> Result<Self> {
        Self::create(width, height, fps).map_err(|e| format!("remote_hevc_unsupported: {e}"))
    }

    fn create(width: u32, height: u32, fps: u32) -> Result<Self> {
        if width < 2
            || height < 2
            || width % 2 != 0
            || height % 2 != 0
            || width > 4096
            || height > 4096
            || width as u64 * height as u64 > 8_388_608
        {
            return Err("invalid NV12 dimensions".into());
        }
        let fps = fps.clamp(1, 60);
        let runtime = Runtime::new()?;
        let activations = unsafe {
            let filter = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_HEVC,
            };
            let mut raw = ptr::null_mut();
            let mut count = 0;
            // This excludes the inbox software encoder and all software fallback.
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&filter),
                &mut raw,
                &mut count,
            )
            .at("MFTEnumEx.hardware.HEVC")?;
            if raw.is_null() {
                Vec::new()
            } else {
                let values = std::slice::from_raw_parts_mut(raw, count as usize)
                    .iter_mut()
                    .filter_map(Option::take)
                    .collect::<Vec<_>>();
                CoTaskMemFree(Some(raw.cast()));
                values
            }
        };
        let mut reasons = Vec::new();
        for activation in activations {
            let configured = unsafe { Self::configure(&activation, width, height, fps) };
            match configured {
                Ok((mft, name, input_id, output_id)) => {
                    return Ok(Self {
                        mft,
                        name,
                        width,
                        height,
                        fps,
                        input_id,
                        output_id,
                        input_ready: 0,
                        sequence: 0,
                        start: Instant::now(),
                        parameter_sets: Default::default(),
                        _runtime: runtime,
                    })
                }
                Err(reason) => {
                    unsafe {
                        let _ = activation.ShutdownObject();
                    }
                    reasons.push(reason);
                }
            }
        }
        Err(if reasons.is_empty() {
            "no hardware HEVC MFT registered".into()
        } else {
            reasons.join("; ")
        })
    }

    unsafe fn configure(
        activation: &IMFActivate,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<(Transform, String, u32, u32)> {
        let hardware_url = string_attr(activation, &MFT_ENUM_HARDWARE_URL_Attribute)?;
        if hardware_url.is_empty() {
            return Err("hardware MFT has no hardware URL".into());
        }
        let mut name = string_attr(activation, &MFT_FRIENDLY_NAME_Attribute)?;
        while name.len() > 512 {
            name.pop();
        }
        let transform = activation
            .ActivateObject::<IMFTransform>()
            .at("ActivateObject")?;
        let attrs = transform.GetAttributes().at("GetAttributes")?;
        if attrs
            .GetUINT32(&MF_TRANSFORM_ASYNC)
            .at("MF_TRANSFORM_ASYNC")?
            == 0
        {
            return Err("hardware encoder did not expose async MFT".into());
        }
        attrs
            .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
            .at("async_unlock")?;
        let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
        let codec: ICodecAPI = transform.cast().at("ICodecAPI")?;
        // With one input in flight an encoder that requires reordering could
        // stall. Require real low-latency support, rather than guessing it.
        codec
            .SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true))
            .at("low_latency")?;
        let bitrate =
            (width as u64 * height as u64 * fps as u64 / 15).clamp(700_000, 12_000_000) as u32;
        let _ = codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate));
        let _ = codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(30u32));
        // Some hardware encoders reject this optional setting with E_INVALIDARG.
        // Low-latency mode is the required setting and actual IRAPs are parsed.
        let _ = codec.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &VARIANT::from(0u32));
        let (mut input_ids, mut output_ids) = ([0], [0]);
        let _ = transform.GetStreamIDs(&mut input_ids, &mut output_ids);
        let (input_id, output_id) = (input_ids[0], output_ids[0]);
        let output = media_type(width, height, fps, &MFVideoFormat_HEVC)?;
        output
            .SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
            .at("output.bitrate")?;
        output
            .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32)
            .at("output.profile")?;
        transform
            .SetOutputType(output_id, &output, 0)
            .at("SetOutputType.HEVC")?;
        let input = media_type(width, height, fps, &MFVideoFormat_NV12)?;
        input
            .SetUINT32(&MF_MT_DEFAULT_STRIDE, width)
            .at("input.stride")?;
        input
            .SetUINT32(&MF_MT_SAMPLE_SIZE, width * height * 3 / 2)
            .at("input.size")?;
        input
            .SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)
            .at("input.fixed")?;
        input
            .SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
            .at("input.independent")?;
        transform
            .SetInputType(input_id, &input, 0)
            .at("SetInputType.NV12")?;
        let events: IMFMediaEventGenerator = transform.cast().at("IMFMediaEventGenerator")?;
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .at("begin_streaming")?;
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .at("start_of_stream")?;
        Ok((
            Transform {
                transform,
                events,
                event_wait: EventWait::new(),
                codec,
                activation: activation.clone(),
            },
            name,
            input_id,
            output_id,
        ))
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn encode(&mut self, nv12: &[u8], force_keyframe: bool) -> Result<Frame> {
        if nv12.len() != (self.width * self.height * 3 / 2) as usize {
            return Err("remote_hevc_encode: invalid tightly packed NV12 buffer".into());
        }
        let started = Instant::now();
        let deadline = started
            + if self.sequence == 0 {
                Duration::from_millis(1200)
            } else {
                Duration::from_millis(250)
            };
        let pts_us = self.start.elapsed().as_micros().min(i64::MAX as u128 / 10) as u64;
        let mut submitted = false;
        unsafe {
            if force_keyframe && self.sequence > 0 {
                self.mft
                    .codec
                    .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32))
                    .at("force_keyframe")?;
            }
            // Stream requirements become valid after both media types are set.
            // Query them before allocating the caller-owned input sample.
            let mut input_info = MFT_INPUT_STREAM_INFO::default();
            self.mft
                .transform
                .GetInputStreamInfo(self.input_id, &mut input_info)
                .at("GetInputStreamInfo")?;
            let sample = sample(
                nv12,
                pts_us as i64 * 10,
                10_000_000 / self.fps as i64,
                &input_info,
            )?;
            while Instant::now() < deadline {
                if self.input_ready > 0 && !submitted {
                    self.mft
                        .transform
                        .ProcessInput(self.input_id, &sample, 0)
                        .at("ProcessInput")?;
                    self.input_ready -= 1;
                    submitted = true;
                }
                let events = &self.mft.events;
                let Some(completion) = self
                    .mft
                    .event_wait
                    .wait(deadline, |callback| events.BeginGetEvent(callback, None))?
                else {
                    break;
                };
                let event = events.EndGetEvent(&completion.0).at("EndGetEvent")?;
                {
                    let status = event.GetStatus().at("event.status")?;
                    if status.is_err() {
                        return Err(error("event.failure", status.into()));
                    }
                    let event_type = event.GetType().at("event.type")?;
                    if event_type == METransformNeedInput.0 as u32 {
                        self.input_ready = self.input_ready.saturating_add(1).min(32);
                    } else if event_type == METransformHaveOutput.0 as u32 {
                        let Some(output) = self.output()? else {
                            continue;
                        };
                        if !submitted {
                            return Err("remote_hevc_encode: unexpected delayed output".into());
                        }
                        let buffer = output.ConvertToContiguousBuffer().at("output.contiguous")?;
                        let length = buffer.GetCurrentLength().at("output.length")? as usize;
                        if length == 0 || length > MAX_ACCESS_UNIT {
                            return Err("remote_hevc_encode: invalid access unit size".into());
                        }
                        let mut data = ptr::null_mut();
                        buffer.Lock(&mut data, None, None).at("output.lock")?;
                        let mut bytes = std::slice::from_raw_parts(data, length).to_vec();
                        buffer.Unlock().at("output.unlock")?;
                        let keyframe = complete_access_unit(&mut bytes, &mut self.parameter_sets)?;
                        if (force_keyframe || self.sequence == 0) && !keyframe {
                            return Err(
                                "remote_hevc_encode: requested keyframe was not an IRAP".into()
                            );
                        }
                        self.sequence += 1;
                        return Ok(Frame {
                            data: bytes,
                            keyframe,
                            sequence: self.sequence,
                            pts_us,
                            encode_us: started.elapsed().as_micros().min(u32::MAX as u128) as u32,
                        });
                    }
                }
            }
        }
        Err(format!(
            "remote_hevc_encode: timed out awaiting {}",
            if submitted {
                "hardware output"
            } else {
                "input credit"
            }
        ))
    }

    unsafe fn output(&self) -> Result<Option<IMFSample>> {
        let info = self
            .mft
            .transform
            .GetOutputStreamInfo(self.output_id)
            .at("GetOutputStreamInfo")?;
        let allocated = info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
            != 0;
        let provided = if allocated {
            None
        } else {
            if info.cbSize as usize > MAX_ACCESS_UNIT {
                return Err("remote_hevc_encode: oversized output buffer".into());
            }
            let sample = MFCreateSample().at("output.sample")?;
            let buffer = media_buffer(
                info.cbSize.max(4 * 1024 * 1024),
                info.cbAlignment,
                "output.buffer",
            )?;
            sample.AddBuffer(&buffer).at("output.add_buffer")?;
            Some(sample)
        };
        let mut output = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: self.output_id,
            pSample: ManuallyDrop::new(provided),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];
        let mut status = 0;
        let result = self
            .mft
            .transform
            .ProcessOutput(0, &mut output, &mut status);
        let sample = ManuallyDrop::take(&mut output[0].pSample);
        drop(ManuallyDrop::take(&mut output[0].pEvents));
        match result {
            Ok(()) => Ok(sample),
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(e) => Err(error("ProcessOutput", e)),
        }
    }
}

unsafe fn string_attr(attrs: &IMFAttributes, key: &GUID) -> Result<String> {
    let length = attrs.GetStringLength(key).at("attribute.string_length")?;
    if length > 4096 {
        return Err("oversized encoder attribute".into());
    }
    let mut text = vec![0u16; length as usize + 1];
    attrs
        .GetString(key, &mut text, None)
        .at("attribute.string")?;
    Ok(String::from_utf16_lossy(&text[..length as usize]))
}

unsafe fn media_type(width: u32, height: u32, fps: u32, subtype: &GUID) -> Result<IMFMediaType> {
    let mt = MFCreateMediaType().at("media_type")?;
    mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .at("type.major")?;
    mt.SetGUID(&MF_MT_SUBTYPE, subtype).at("type.subtype")?;
    mt.SetUINT64(&MF_MT_FRAME_SIZE, (width as u64) << 32 | height as u64)
        .at("type.size")?;
    mt.SetUINT64(&MF_MT_FRAME_RATE, (fps as u64) << 32 | 1)
        .at("type.fps")?;
    mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, 1u64 << 32 | 1)
        .at("type.aspect")?;
    mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        .at("type.progressive")?;
    Ok(mt)
}

fn alignment_mask(alignment: u32) -> Result<Option<u32>> {
    if alignment == 0 {
        return Ok(None);
    }
    if !alignment.is_power_of_two() {
        return Err(format!(
            "remote_hevc_encode: invalid MFT alignment {alignment}"
        ));
    }
    // MFT stream info reports bytes. MFCreateAlignedMemoryBuffer instead uses
    // MF_*_BYTE_ALIGNMENT masks: 16-byte alignment is 0x0f, not 16.
    Ok(Some(alignment - 1))
}

unsafe fn media_buffer(capacity: u32, alignment: u32, stage: &str) -> Result<IMFMediaBuffer> {
    match alignment_mask(alignment)? {
        Some(mask) => MFCreateAlignedMemoryBuffer(capacity, mask).at(stage),
        None => MFCreateMemoryBuffer(capacity).at(stage),
    }
}

unsafe fn sample(
    nv12: &[u8],
    pts: i64,
    duration: i64,
    info: &MFT_INPUT_STREAM_INFO,
) -> Result<IMFSample> {
    // cbSize is a minimum allocation size; CurrentLength remains the actual
    // packed NV12 payload length, so padding is never submitted as pixel data.
    let buffer = media_buffer(
        (nv12.len() as u32).max(info.cbSize),
        info.cbAlignment,
        "input.buffer",
    )?;
    let mut data = ptr::null_mut();
    buffer.Lock(&mut data, None, None).at("input.lock")?;
    ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
    buffer.Unlock().at("input.unlock")?;
    buffer
        .SetCurrentLength(nv12.len() as u32)
        .at("input.length")?;
    let sample = MFCreateSample().at("input.sample")?;
    sample.AddBuffer(&buffer).at("input.add_buffer")?;
    sample.SetSampleTime(pts).at("input.time")?;
    sample.SetSampleDuration(duration).at("input.duration")?;
    Ok(sample)
}

/// Returns Annex B NAL ranges, including each start code. No length-prefixed
/// representation is accepted, so a mislabeled encoder cannot feed WebCodecs.
fn nal_ranges(bytes: &[u8]) -> Result<Vec<(usize, usize, u8)>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let prefix = if bytes[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        if i + prefix + 1 >= bytes.len() {
            return Err("remote_hevc_encode: truncated NAL".into());
        }
        starts.push((i, (bytes[i + prefix] >> 1) & 63));
        i += prefix + 2;
    }
    if starts.first().is_none_or(|entry| entry.0 != 0) {
        return Err("remote_hevc_encode: output is not Annex B".into());
    }
    Ok(starts
        .iter()
        .enumerate()
        .map(|(i, &(start, kind))| {
            (
                start,
                starts.get(i + 1).map(|s| s.0).unwrap_or(bytes.len()),
                kind,
            )
        })
        .collect())
}

fn complete_access_unit(bytes: &mut Vec<u8>, sets: &mut [Vec<u8>; 3]) -> Result<bool> {
    let ranges = nal_ranges(bytes)?;
    let keyframe = ranges.iter().any(|r| (16..=23).contains(&r.2));
    let mut present = [false; 3];
    for &(start, end, kind) in &ranges {
        if (32..=34).contains(&kind) {
            let index = (kind - 32) as usize;
            sets[index] = bytes[start..end].to_vec();
            present[index] = true;
        }
    }
    if keyframe {
        if sets.iter().any(Vec::is_empty) {
            return Err("remote_hevc_encode: IRAP missing parameter sets".into());
        }
        if present.iter().any(|p| !p) {
            let mut full =
                Vec::with_capacity(bytes.len() + sets.iter().map(Vec::len).sum::<usize>());
            for (index, set) in sets.iter().enumerate() {
                if !present[index] {
                    full.extend_from_slice(set);
                }
            }
            full.append(bytes);
            *bytes = full;
        }
    }
    Ok(keyframe)
}

/// XHV1: a small metadata prefix followed by the encoder name and one Annex B AU.
/// Session/revision ownership is checked by the caller and the transport envelope.
pub(super) fn packet(
    name: &str,
    coded: (u32, u32),
    source: (u32, u32),
    fps: u32,
    capture_us: u32,
    native_us: u32,
    previous_sequence: u64,
    frame: Option<Frame>,
) -> Vec<u8> {
    let name = name.as_bytes();
    let header = HEADER_LEN + name.len();
    let (sequence, pts, encode_us, flags, payload) = match frame {
        Some(f) => (
            f.sequence,
            f.pts_us,
            f.encode_us,
            2 | u32::from(f.keyframe),
            f.data,
        ),
        None => (previous_sequence, 0, 0, 2 | 4, Vec::new()),
    };
    let mut bytes = vec![0; header];
    bytes[..4].copy_from_slice(b"XHV1");
    bytes[4..8].copy_from_slice(&(header as u32).to_le_bytes());
    bytes[8..12].copy_from_slice(&coded.0.to_le_bytes());
    bytes[12..16].copy_from_slice(&coded.1.to_le_bytes());
    bytes[16..20].copy_from_slice(&source.0.to_le_bytes());
    bytes[20..24].copy_from_slice(&source.1.to_le_bytes());
    bytes[24..32].copy_from_slice(&sequence.to_le_bytes());
    bytes[32..40].copy_from_slice(&pts.to_le_bytes());
    bytes[40..44].copy_from_slice(&capture_us.to_le_bytes());
    bytes[44..48].copy_from_slice(&encode_us.to_le_bytes());
    bytes[48..52].copy_from_slice(&flags.to_le_bytes());
    bytes[52..56].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[56..58].copy_from_slice(&(name.len() as u16).to_le_bytes());
    bytes[60..64].copy_from_slice(&fps.to_le_bytes());
    bytes[64..68].copy_from_slice(&native_us.to_le_bytes());
    bytes[HEADER_LEN..].copy_from_slice(name);
    bytes.extend_from_slice(&payload);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_wait_uses_real_mf_callbacks_and_keeps_one_subscription_after_timeout() {
        // Real MF work-queue delivery and EndGetEvent on the capture thread;
        // no encoder enumeration, GPU, screen capture, or performance benchmark.
        let _runtime = Runtime::new().unwrap();
        unsafe {
            let queue = MFCreateEventQueue().unwrap();
            let mut wait = EventWait::new();
            assert!(wait
                .wait(Instant::now() + Duration::from_millis(10), |callback| {
                    queue.BeginGetEvent(callback, None)
                })
                .unwrap()
                .is_none());
            assert!(wait.pending);
            for kind in [METransformNeedInput, METransformHaveOutput] {
                queue
                    .QueueEventParamUnk(
                        kind.0 as u32,
                        &GUID::zeroed(),
                        windows::core::HRESULT(0),
                        None,
                    )
                    .unwrap();
            }
            {
                let completion = wait
                    .wait(Instant::now() + Duration::from_secs(1), |_| {
                        panic!("a timed-out subscription must not be started twice")
                    })
                    .unwrap()
                    .unwrap();
                let event = queue.EndGetEvent(&completion.0).unwrap();
                assert_eq!(event.GetType().unwrap(), METransformNeedInput.0 as u32);
                assert!(!wait.pending);
            }
            {
                let completion = wait
                    .wait(Instant::now() + Duration::from_secs(1), |callback| {
                        queue.BeginGetEvent(callback, None)
                    })
                    .unwrap()
                    .unwrap();
                let event = queue.EndGetEvent(&completion.0).unwrap();
                assert_eq!(event.GetType().unwrap(), METransformHaveOutput.0 as u32);
            }
            wait.slot.close();
            queue.Shutdown().unwrap();
        }
    }

    #[test]
    fn event_wait_close_releases_queued_results_and_ignores_late_callbacks() {
        let _runtime = Runtime::new().unwrap();
        unsafe {
            let wait = EventWait::new();
            let weak = Arc::downgrade(&wait.slot);
            let callback = wait.callback.clone();
            let completion = MFCreateAsyncResult(None, &callback, None).unwrap();
            callback.Invoke(&completion).unwrap();
            assert!(wait.slot.state.lock().unwrap().completion.is_some());
            wait.slot.close();
            assert!(wait.slot.state.lock().unwrap().completion.is_none());
            callback.Invoke(&completion).unwrap();
            assert!(wait.slot.state.lock().unwrap().completion.is_none());
            assert!(wait
                .slot
                .wait(Instant::now())
                .err()
                .unwrap()
                .contains("closed"));
            drop(wait);
            // A queued IMFAsyncResult can retain its callback; close must break
            // that cycle before MF shutdown even when delivery raced with stop.
            drop(completion);
            drop(callback);
            assert!(weak.upgrade().is_none());
        }
    }

    #[test]
    fn event_wait_rejects_duplicate_callbacks_without_queue_growth() {
        let _runtime = Runtime::new().unwrap();
        unsafe {
            let wait = EventWait::new();
            let completion = MFCreateAsyncResult(None, &wait.callback, None).unwrap();
            wait.callback.Invoke(&completion).unwrap();
            wait.callback.Invoke(&completion).unwrap();
            assert!(wait
                .slot
                .wait(Instant::now())
                .err()
                .unwrap()
                .contains("invalid"));
            wait.slot.close();
        }
    }

    #[test]
    fn mf_buffers_honor_stream_alignment_and_minimum_input_capacity() {
        assert_eq!(alignment_mask(0).unwrap(), None);
        assert_eq!(alignment_mask(1).unwrap(), Some(MF_1_BYTE_ALIGNMENT));
        assert_eq!(alignment_mask(16).unwrap(), Some(MF_16_BYTE_ALIGNMENT));
        assert_eq!(alignment_mask(512).unwrap(), Some(MF_512_BYTE_ALIGNMENT));
        assert!(alignment_mask(24).is_err());
        // Exercise actual MF system-memory allocation and sample wrapping,
        // without enumerating an encoder, creating a GPU device, or capture.
        let _runtime = Runtime::new().unwrap();
        let nv12 = [17u8, 32, 64, 128, 240, 128];
        for alignment in [0, 1, 16, 64, 512] {
            unsafe {
                let info = MFT_INPUT_STREAM_INFO {
                    cbSize: 4096,
                    cbAlignment: alignment,
                    ..Default::default()
                };
                let sample = sample(&nv12, 123, 333_333, &info).unwrap();
                let input = sample.GetBufferByIndex(0).unwrap();
                assert!(input.GetMaxLength().unwrap() >= info.cbSize);
                assert_eq!(input.GetCurrentLength().unwrap(), nv12.len() as u32);
                let mut data = ptr::null_mut();
                input.Lock(&mut data, None, None).unwrap();
                let aligned = data as usize % alignment.max(1) as usize == 0;
                let copied = std::slice::from_raw_parts(data, nv12.len()).to_vec();
                input.Unlock().unwrap();
                assert!(aligned, "input alignment {alignment}");
                assert_eq!(copied, nv12);
                assert_eq!(sample.GetSampleTime().unwrap(), 123);
                let output = media_buffer(8192, alignment, "output.buffer").unwrap();
                output.Lock(&mut data, None, None).unwrap();
                let aligned = data as usize % alignment.max(1) as usize == 0;
                output.Unlock().unwrap();
                assert!(aligned, "output alignment {alignment}");
            }
        }
    }

    #[test]
    fn access_units_require_annex_b_and_make_every_irap_independent() {
        let nal = |kind: u8| vec![0, 0, 0, 1, kind << 1, 1, 42];
        let mut sets = Default::default();
        let mut first = [nal(32), nal(33), nal(34), nal(19)].concat();
        assert!(complete_access_unit(&mut first, &mut sets).unwrap());
        let mut forced = nal(19);
        assert!(complete_access_unit(&mut forced, &mut sets).unwrap());
        assert_eq!(forced, first);
        let mut delta = nal(1);
        assert!(!complete_access_unit(&mut delta, &mut sets).unwrap());
        assert!(complete_access_unit(&mut nal(19), &mut Default::default()).is_err());
        assert!(complete_access_unit(&mut vec![0, 0, 0, 6, 38, 1, 9], &mut sets).is_err());
    }

    #[test]
    fn packet_marks_actual_keyframe_and_unchanged_without_fake_sequence() {
        let payload = vec![0, 0, 1, 38, 1, 7];
        let frame = Frame {
            data: payload.clone(),
            keyframe: true,
            sequence: 7,
            pts_us: 123,
            encode_us: 9,
        };
        let encoded = packet("GPU", (1920, 1080), (2560, 1440), 30, 8, 22, 6, Some(frame));
        assert_eq!(&encoded[..4], b"XHV1");
        assert_eq!(u32::from_le_bytes(encoded[4..8].try_into().unwrap()), 71);
        assert_eq!(&encoded[68..71], b"GPU");
        assert_eq!(&encoded[71..], payload);
        assert_eq!(u32::from_le_bytes(encoded[64..68].try_into().unwrap()), 22);
        assert_eq!(u32::from_le_bytes(encoded[48..52].try_into().unwrap()), 3);
        let idle = packet("GPU", (1920, 1080), (2560, 1440), 30, 8, 22, 7, None);
        assert_eq!(u64::from_le_bytes(idle[24..32].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(idle[48..52].try_into().unwrap()), 6);
        assert_eq!(u32::from_le_bytes(idle[52..56].try_into().unwrap()), 0);
    }
}
