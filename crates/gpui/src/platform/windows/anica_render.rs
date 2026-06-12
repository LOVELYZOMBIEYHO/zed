use crate::{Bounds, ContentMask, ScaledPixels};
use windows::Win32::Graphics::{
    Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D},
    Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB},
};

/// Public anica wrapper for the GPUI-owned D3D11 device pair.
#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
pub struct D3d11Devices_anica {
    /// The D3D11 device used by GPUI's Windows renderer.
    pub device: ID3D11Device,
    /// The immediate context used by GPUI's Windows renderer.
    pub device_context: ID3D11DeviceContext,
}

/// Extended surface parameters for native BGRA video frame rendering.
#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
pub struct SurfaceExParams_anica {
    /// Overall opacity [0.0 .. 1.0].
    pub opacity: f32,
    /// Scale factor (1.0 = original size).
    pub scale: f32,
    /// Rotation in degrees (applied around frame centre).
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

/// Platform-native BGRA frame storage used by `paint_bgra_frame_anica`.
#[derive(Clone, Debug)]
pub enum BgraFrameSurface {
    /// A Direct3D 11 BGRA texture exposed through DXGI.
    D3d11Texture(ID3D11Texture2D),
}

impl BgraFrameSurface {
    /// Returns true when the wrapped platform surface is a BGRA texture.
    pub fn is_bgra(&self) -> bool {
        matches!(
            self.pixel_format(),
            DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        )
    }

    /// Returns the DXGI format of the wrapped texture.
    pub fn pixel_format(&self) -> DXGI_FORMAT {
        self.desc().Format
    }

    /// Returns the source texture width in pixels.
    pub fn width(&self) -> u32 {
        self.desc().Width
    }

    /// Returns the source texture height in pixels.
    pub fn height(&self) -> u32 {
        self.desc().Height
    }

    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        match self {
            Self::D3d11Texture(texture) => texture,
        }
    }

    fn desc(&self) -> D3D11_TEXTURE2D_DESC {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            self.texture().GetDesc(&mut desc);
        }
        desc
    }
}

/// Scene primitive for the Windows native BGRA frame path (anica).
#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
pub(crate) struct PaintBgraFrame_anica {
    pub order: crate::scene::DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub surface: BgraFrameSurface,
    pub params: SurfaceExParams_anica,
}

/// GPU instance data for the Windows BGRA frame shader.
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub(crate) struct BgraFrameBounds_anica {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub opacity: f32,
    pub scale: f32,
    pub rotation_rad: f32,
    pub _pad: f32,
    pub translate_x: f32,
    pub translate_y: f32,
}
