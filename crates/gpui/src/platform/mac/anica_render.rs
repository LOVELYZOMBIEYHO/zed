// =========================================
// =========================================
// crates/gpui-0.2.2-anica-edition/src/platform/mac/anica_render.rs

use crate::{Bounds, ContentMask, DevicePixels, ScaledPixels, Size, size};
use core_foundation::base::TCFType;
use core_video::{
    metal_texture::{CVMetalTexture, CVMetalTextureGetTexture},
    metal_texture_cache::CVMetalTextureCache,
    pixel_buffer::{
        kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    },
};
use foreign_types::ForeignTypeRef;
use metal::MTLPixelFormat;
use std::{mem, ptr};

/// Identifies the NV12 range variant so shader conversion can stay correct.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Nv12SurfaceKind {
    FullRange,
    VideoRange,
}

impl Nv12SurfaceKind {
    /// Encodes the range as a compact shader flag.
    pub(crate) fn shader_flag(self) -> u32 {
        match self {
            Self::FullRange => 0,
            Self::VideoRange => 1,
        }
    }
}

/// Identifies all CoreVideo formats supported by the surface renderer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SurfaceTextureKind {
    Nv12(Nv12SurfaceKind),
    Bgra,
}

impl SurfaceTextureKind {
    /// Encodes the source texture layout for the shared surface fragment shader.
    pub(crate) fn shader_flag(self) -> u32 {
        match self {
            Self::Nv12(kind) => kind.shader_flag(),
            Self::Bgra => 2,
        }
    }
}

/// Classifies supported NV12 pixel formats (420f/420v) for surface rendering.
pub(crate) fn classify_nv12_surface(pixel_format: u32) -> Option<Nv12SurfaceKind> {
    if pixel_format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
        Some(Nv12SurfaceKind::FullRange)
    } else if pixel_format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
        Some(Nv12SurfaceKind::VideoRange)
    } else {
        None
    }
}

/// Classifies CoreVideo pixel formats accepted by GPUI surface rendering.
pub(crate) fn classify_surface_texture(pixel_format: u32) -> Option<SurfaceTextureKind> {
    if pixel_format == kCVPixelFormatType_32BGRA {
        Some(SurfaceTextureKind::Bgra)
    } else {
        classify_nv12_surface(pixel_format).map(SurfaceTextureKind::Nv12)
    }
}

/// Formats a CoreVideo pixel format as a readable fourcc for debug logs.
pub(crate) fn pixel_format_fourcc(pixel_format: u32) -> String {
    let bytes = pixel_format.to_be_bytes();
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() {
                *byte as char
            } else {
                '.'
            }
        })
        .collect()
}

// ─── Anica extended surface rendering ───────────────────────────────────

/// Extended surface parameters for CoreVideo surface rendering with
/// opacity, transform, and mask support.
#[derive(Clone, Debug)]
pub struct SurfaceExParams_anica {
    /// Overall opacity [0.0 .. 1.0].
    pub opacity: f32,
    /// Scale factor (1.0 = original size).
    pub scale: f32,
    /// Rotation in degrees (applied around surface centre).
    pub rotation_deg: f32,
    /// Translation offset in scaled pixels.
    pub translate: crate::Point<ScaledPixels>,
}

impl Default for SurfaceExParams_anica {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            scale: 1.0,
            rotation_deg: 0.0,
            translate: crate::Point {
                x: ScaledPixels(0.0),
                y: ScaledPixels(0.0),
            },
        }
    }
}

/// Scene primitive for the extended CoreVideo surface path (anica).
#[derive(Clone, Debug)]
pub(crate) struct PaintSurface_anica {
    pub order: crate::scene::DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub image_buffer: core_video::pixel_buffer::CVPixelBuffer,
    pub params: SurfaceExParams_anica,
}

/// Platform-native BGRA frame storage used by `paint_bgra_frame_anica`.
#[derive(Clone, Debug)]
pub enum BgraFrameSurface {
    /// A macOS CoreVideo BGRA pixel buffer.
    CvPixelBuffer(core_video::pixel_buffer::CVPixelBuffer),
}

impl BgraFrameSurface {
    /// Returns true when the wrapped platform surface is a BGRA pixel buffer.
    pub fn is_bgra(&self) -> bool {
        match self {
            Self::CvPixelBuffer(buffer) => buffer.get_pixel_format() == kCVPixelFormatType_32BGRA,
        }
    }

    pub(crate) fn pixel_format(&self) -> u32 {
        match self {
            Self::CvPixelBuffer(buffer) => buffer.get_pixel_format(),
        }
    }

    pub(crate) fn into_cv_pixel_buffer(self) -> core_video::pixel_buffer::CVPixelBuffer {
        match self {
            Self::CvPixelBuffer(buffer) => buffer,
        }
    }
}

/// GPU instance data written into the Metal instance buffer for
/// the extended surface shader (matches the Metal struct layout).
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SurfaceBounds_anica {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    /// opacity, scale, rotation_rad, _pad  — vec4 in shader
    pub opacity: f32,
    pub scale: f32,
    pub rotation_rad: f32,
    pub _pad: f32,
    /// translate (x, y) in device pixels
    pub translate_x: f32,
    pub translate_y: f32,
}

/// Shader buffer binding indices for the extended surface pipeline.
#[repr(C)]
pub(crate) enum SurfaceInputIndex_anica {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
    ColorRange = 6,
}

enum SurfaceMetalTextures {
    Nv12 {
        y_texture: CVMetalTexture,
        cb_cr_texture: CVMetalTexture,
    },
    Bgra {
        texture: CVMetalTexture,
    },
}

/// Draws extended CoreVideo surfaces with opacity / transform / mask support.
/// All custom rendering logic lives here to keep GPUI core files minimal.
pub(crate) fn draw_surfaces_anica(
    surfaces: &[PaintSurface_anica],
    surfaces_pipeline_state: &metal::RenderPipelineState,
    unit_vertices: &metal::Buffer,
    core_video_texture_cache: &CVMetalTextureCache,
    instance_buffer_metal: &metal::Buffer,
    instance_buffer_size: usize,
    instance_offset: &mut usize,
    viewport_size: Size<DevicePixels>,
    command_encoder: &metal::RenderCommandEncoderRef,
) -> bool {
    command_encoder.set_render_pipeline_state(surfaces_pipeline_state);
    command_encoder.set_vertex_buffer(
        SurfaceInputIndex_anica::Vertices as u64,
        Some(unit_vertices),
        0,
    );
    command_encoder.set_vertex_bytes(
        SurfaceInputIndex_anica::ViewportSize as u64,
        mem::size_of_val(&viewport_size) as u64,
        &viewport_size as *const Size<DevicePixels> as *const _,
    );

    for surface in surfaces {
        let texture_size = size(
            DevicePixels::from(surface.image_buffer.get_width() as i32),
            DevicePixels::from(surface.image_buffer.get_height() as i32),
        );
        let pixel_format = surface.image_buffer.get_pixel_format();
        // Classify the source buffer so NV12 and BGRA can share the surface path.
        let Some(surface_kind) = classify_surface_texture(pixel_format) else {
            log::warn!(
                "[GPUI][SurfaceAnica] unsupported pixel format={} (0x{pixel_format:08x}), skipping",
                pixel_format_fourcc(pixel_format),
            );
            continue;
        };

        // Create Metal textures directly from the CVPixelBuffer IOSurface.
        let surface_textures = match surface_kind {
            SurfaceTextureKind::Nv12(_) => {
                let y_texture = core_video_texture_cache
                    .create_texture_from_image(
                        surface.image_buffer.as_concrete_TypeRef(),
                        None,
                        MTLPixelFormat::R8Unorm,
                        surface.image_buffer.get_width_of_plane(0),
                        surface.image_buffer.get_height_of_plane(0),
                        0,
                    )
                    .unwrap();
                let cb_cr_texture = core_video_texture_cache
                    .create_texture_from_image(
                        surface.image_buffer.as_concrete_TypeRef(),
                        None,
                        MTLPixelFormat::RG8Unorm,
                        surface.image_buffer.get_width_of_plane(1),
                        surface.image_buffer.get_height_of_plane(1),
                        1,
                    )
                    .unwrap();
                SurfaceMetalTextures::Nv12 {
                    y_texture,
                    cb_cr_texture,
                }
            }
            SurfaceTextureKind::Bgra => {
                let texture = core_video_texture_cache
                    .create_texture_from_image(
                        surface.image_buffer.as_concrete_TypeRef(),
                        None,
                        MTLPixelFormat::BGRA8Unorm,
                        surface.image_buffer.get_width(),
                        surface.image_buffer.get_height(),
                        0,
                    )
                    .unwrap();
                SurfaceMetalTextures::Bgra { texture }
            }
        };

        // Align instance offset to 256 bytes for Metal.
        *instance_offset = (*instance_offset).div_ceil(256) * 256;
        let next_offset = *instance_offset + mem::size_of::<SurfaceBounds_anica>();
        if next_offset > instance_buffer_size {
            return false;
        }

        // Write extended instance data.
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex_anica::Surfaces as u64,
            Some(instance_buffer_metal),
            *instance_offset as u64,
        );
        // Fragment shader reads `surfaces[0].opacity`, so bind the same instance
        // buffer for fragment stage as well.
        command_encoder.set_fragment_buffer(
            SurfaceInputIndex_anica::Surfaces as u64,
            Some(instance_buffer_metal),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex_anica::TextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        match &surface_textures {
            SurfaceMetalTextures::Nv12 {
                y_texture,
                cb_cr_texture,
            } => {
                command_encoder.set_fragment_texture(
                    SurfaceInputIndex_anica::YTexture as u64,
                    unsafe {
                        let texture = CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef());
                        Some(metal::TextureRef::from_ptr(texture as *mut _))
                    },
                );
                command_encoder.set_fragment_texture(
                    SurfaceInputIndex_anica::CbCrTexture as u64,
                    unsafe {
                        let texture = CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef());
                        Some(metal::TextureRef::from_ptr(texture as *mut _))
                    },
                );
            }
            SurfaceMetalTextures::Bgra { texture } => {
                command_encoder.set_fragment_texture(
                    SurfaceInputIndex_anica::YTexture as u64,
                    unsafe {
                        let texture = CVMetalTextureGetTexture(texture.as_concrete_TypeRef());
                        Some(metal::TextureRef::from_ptr(texture as *mut _))
                    },
                );
                command_encoder.set_fragment_texture(
                    SurfaceInputIndex_anica::CbCrTexture as u64,
                    unsafe {
                        let texture = CVMetalTextureGetTexture(texture.as_concrete_TypeRef());
                        Some(metal::TextureRef::from_ptr(texture as *mut _))
                    },
                );
            }
        }
        // Pass source layout/range flag for direct BGRA or YUV->RGB shader selection.
        let color_range = surface_kind.shader_flag();
        command_encoder.set_fragment_bytes(
            SurfaceInputIndex_anica::ColorRange as u64,
            mem::size_of_val(&color_range) as u64,
            &color_range as *const u32 as *const _,
        );

        // Convert degrees to radians for shader.
        let rotation_rad = surface.params.rotation_deg.to_radians();

        unsafe {
            let buffer_contents = (instance_buffer_metal.contents() as *mut u8)
                .add(*instance_offset)
                as *mut SurfaceBounds_anica;
            ptr::write(
                buffer_contents,
                SurfaceBounds_anica {
                    bounds: surface.bounds,
                    content_mask: surface.content_mask.clone(),
                    opacity: surface.params.opacity,
                    scale: surface.params.scale,
                    rotation_rad,
                    _pad: 0.0,
                    translate_x: surface.params.translate.x.0,
                    translate_y: surface.params.translate.y.0,
                },
            );
        }

        command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        *instance_offset = next_offset;
    }
    true
}
