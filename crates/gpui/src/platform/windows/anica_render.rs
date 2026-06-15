use crate::{Bounds, ContentMask, ScaledPixels};
use windows::Win32::Graphics::{
    Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D},
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
    /// A Direct3D 11 BGRA texture and its cached shader resource view.
    D3d11Texture {
        /// The D3D11 texture.
        texture: ID3D11Texture2D,
        /// Cached shader resource view for sampling the texture.
        shader_resource_view: ID3D11ShaderResourceView,
        /// Texture width in pixels.
        width: u32,
        /// Texture height in pixels.
        height: u32,
        /// DXGI format of the texture.
        format: DXGI_FORMAT,
    },
}

impl BgraFrameSurface {
    /// Open a DXGI shared handle on the given GPUI D3D11 device and wrap it as a
    /// `BgraFrameSurface`. Returns `None` if the handle cannot be opened or the
    /// shader resource view cannot be created.
    pub fn from_shared_handle(
        devices: &D3d11Devices_anica,
        shared_handle: isize,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Option<Self> {
        Self::from_shared_handle_internal(devices, shared_handle, width, height, format)
    }

    /// Convenience constructor for the common BGRA8_UNORM preview surface format.
    pub fn from_shared_handle_bgra(
        devices: &D3d11Devices_anica,
        shared_handle: isize,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        Self::from_shared_handle_internal(
            devices,
            shared_handle,
            width,
            height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )
    }

    fn from_shared_handle_internal(
        devices: &D3d11Devices_anica,
        shared_handle: isize,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Option<Self> {
        if shared_handle == 0 {
            return None;
        }
        unsafe {
            use windows::Win32::Foundation::HANDLE;
            let mut texture: Option<ID3D11Texture2D> = None;
            devices
                .device
                .OpenSharedResource(
                    HANDLE(shared_handle as *mut core::ffi::c_void),
                    &mut texture,
                )
                .ok()?;
            let texture = texture?;
            let mut desc = Default::default();
            texture.GetDesc(&mut desc);
            if desc.Width != width
                || desc.Height != height
                || desc.Format != format
                || desc.MipLevels != 1
                || desc.ArraySize != 1
            {
                return None;
            }
            let mut shader_resource_view = None;
            devices
                .device
                .CreateShaderResourceView(&texture, None, Some(&mut shader_resource_view))
                .ok()?;
            Some(Self::D3d11Texture {
                texture,
                shader_resource_view: shader_resource_view?,
                width,
                height,
                format,
            })
        }
    }

    /// Returns true when the wrapped platform surface is a BGRA texture.
    pub fn is_bgra(&self) -> bool {
        matches!(
            self.pixel_format(),
            DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        )
    }

    /// Returns the DXGI format of the wrapped texture.
    pub fn pixel_format(&self) -> DXGI_FORMAT {
        match self {
            Self::D3d11Texture { format, .. } => *format,
        }
    }

    /// Returns the source texture width in pixels.
    pub fn width(&self) -> u32 {
        match self {
            Self::D3d11Texture { width, .. } => *width,
        }
    }

    /// Returns the source texture height in pixels.
    pub fn height(&self) -> u32 {
        match self {
            Self::D3d11Texture { height, .. } => *height,
        }
    }

    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        match self {
            Self::D3d11Texture { texture, .. } => texture,
        }
    }

    pub(crate) fn shader_resource_view(&self) -> &ID3D11ShaderResourceView {
        match self {
            Self::D3d11Texture {
                shader_resource_view,
                ..
            } => shader_resource_view,
        }
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
