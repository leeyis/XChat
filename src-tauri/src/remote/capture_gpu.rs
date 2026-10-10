//! GPU scaling before readback keeps the CPU out of the full-screen resize loop.
//! Drivers without a suitable D3D11 video processor keep the CPU fallback.
use windows::{
    core::Interface,
    Win32::{
        Foundation::RECT,
        Graphics::{Direct3D11::*, Dxgi::Common::*},
    },
};

pub(super) struct Scaler {
    context: ID3D11DeviceContext,
    video_context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    input: ID3D11Texture2D,
    input_view: ID3D11VideoProcessorInputView,
    output: ID3D11Texture2D,
    output_view: ID3D11VideoProcessorOutputView,
}

impl Scaler {
    pub(super) fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: D3D11_TEXTURE2D_DESC,
        target: (u32, u32),
    ) -> windows::core::Result<Self> {
        Self::with_format(device, context, source, target, DXGI_FORMAT_R8G8B8A8_UNORM)
    }

    fn with_format(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: D3D11_TEXTURE2D_DESC,
        target: (u32, u32),
        format: DXGI_FORMAT,
    ) -> windows::core::Result<Self> {
        unsafe {
            let video: ID3D11VideoDevice = device.cast().map_err(|e| stage("video device", e))?;
            let video_context: ID3D11VideoContext =
                context.cast().map_err(|e| stage("video context", e))?;
            let description = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: source.Width,
                InputHeight: source.Height,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: target.0,
                OutputHeight: target.1,
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
            };
            let enumerator = video
                .CreateVideoProcessorEnumerator(&description)
                .map_err(|e| stage("processor enumerator", e))?;
            let processor = video
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|e| stage("video processor", e))?;
            let input_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_DEFAULT,
                // Video-processor input views require DEFAULT resources with
                // a video/render bind flag (or zero), not SHADER_RESOURCE alone.
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                ..source
            };
            let mut input = None;
            device
                .CreateTexture2D(&input_desc, None, Some(&mut input))
                .map_err(|e| stage("input texture", e))?;
            let input = input.unwrap();
            let mut input_view = None;
            video
                .CreateVideoProcessorInputView(
                    &input,
                    &enumerator,
                    &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                        FourCC: 0,
                        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_VPIV {
                                MipSlice: 0,
                                ArraySlice: 0,
                            },
                        },
                    },
                    Some(&mut input_view),
                )
                .map_err(|e| stage("input view", e))?;
            let output_desc = D3D11_TEXTURE2D_DESC {
                Width: target.0,
                Height: target.1,
                Format: format,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                ..input_desc
            };
            let mut output = None;
            device
                .CreateTexture2D(&output_desc, None, Some(&mut output))
                .map_err(|e| stage("output texture", e))?;
            let output = output.unwrap();
            let mut output_view = None;
            video
                .CreateVideoProcessorOutputView(
                    &output,
                    &enumerator,
                    &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                        },
                    },
                    Some(&mut output_view),
                )
                .map_err(|e| stage("output view", e))?;
            let src = RECT {
                left: 0,
                top: 0,
                right: source.Width as i32,
                bottom: source.Height as i32,
            };
            let dst = RECT {
                left: 0,
                top: 0,
                right: target.0 as i32,
                bottom: target.1 as i32,
            };
            video_context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            video_context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&src));
            video_context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&dst));
            video_context.VideoProcessorSetOutputTargetRect(&processor, true, Some(&dst));
            video_context.VideoProcessorSetOutputAlphaFillMode(
                &processor,
                D3D11_VIDEO_PROCESSOR_ALPHA_FILL_MODE_OPAQUE,
                0,
            );
            Ok(Self {
                context: context.clone(),
                video_context,
                processor,
                input,
                input_view: input_view.unwrap(),
                output,
                output_view: output_view.unwrap(),
            })
        }
    }

    pub(super) fn scale(
        &self,
        source: &ID3D11Texture2D,
    ) -> windows::core::Result<&ID3D11Texture2D> {
        unsafe {
            self.context.CopyResource(&self.input, source);
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(self.input_view.clone())),
                ..Default::default()
            };
            let result = self.video_context.VideoProcessorBlt(
                &self.processor,
                &self.output_view,
                0,
                std::slice::from_ref(&stream),
            );
            // windows-rs represents COM fields in this C struct as ManuallyDrop.
            // Release our temporary AddRef even if the driver rejects the blit.
            std::mem::ManuallyDrop::drop(&mut stream.pInputSurface);
            result?;
            Ok(&self.output)
        }
    }
}

/// GPU scale/color-conversion to NV12 before the explicit staging readback.
/// A single request owns the output until its bytes have been copied to MF.
pub(super) struct Nv12 {
    scaler: Scaler,
    staging: ID3D11Texture2D,
    context: ID3D11DeviceContext,
    target: (u32, u32),
}

impl Nv12 {
    pub(super) fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: D3D11_TEXTURE2D_DESC,
        target: (u32, u32),
    ) -> windows::core::Result<Self> {
        let scaler = Scaler::with_format(device, context, source, target, DXGI_FORMAT_NV12)?;
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            scaler.output.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .map_err(|e| stage("NV12 staging texture", e))?;
            Ok(Self {
                scaler,
                staging: staging.unwrap(),
                context: context.clone(),
                target,
            })
        }
    }

    pub(super) fn readback(&self, source: &ID3D11Texture2D) -> Result<Vec<u8>, String> {
        unsafe {
            let output = self.scaler.scale(source).map_err(|e| e.to_string())?;
            self.context.CopyResource(&self.staging, output);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| e.to_string())?;
            let (width, height) = (self.target.0 as usize, self.target.1 as usize);
            let result = if mapped.pData.is_null() || (mapped.RowPitch as usize) < width {
                Err("invalid GPU NV12 staging buffer".into())
            } else {
                // NV12 Y rows and interleaved UV rows share RowPitch. Padding
                // belongs to the driver and is never copied into MF input.
                let stride = mapped.RowPitch as usize;
                let rows = height + height / 2;
                let source = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), stride * rows);
                let mut bytes = vec![0; width * rows];
                for (src, dst) in source
                    .chunks_exact(stride)
                    .zip(bytes.chunks_exact_mut(width))
                {
                    dst.copy_from_slice(&src[..width]);
                }
                Ok(bytes)
            };
            self.context.Unmap(&self.staging, 0);
            result
        }
    }
}

fn stage(name: &str, error: windows::core::Error) -> windows::core::Error {
    windows::core::Error::new(error.code(), format!("{name}: {error}"))
}
