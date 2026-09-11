//! Album-art decoding via Windows Imaging Component.
//!
//! Runs on the UI thread: the resulting Direct2D bitmap is created there, and
//! decoding ~40 KB of JPEG costs well under a millisecond once per track.

use crate::render::ArtBitmap;
use windows::{
    core::*,
    Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeMedianCut,
            WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::Shell::SHCreateMemStream,
    },
};

pub struct Wic(IWICImagingFactory);

impl Wic {
    pub fn new() -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self(factory))
        }
    }

    /// Decodes compressed art bytes into a square, premultiplied BGRA bitmap.
    ///
    /// The destination format must be PBGRA (premultiplied) to match the
    /// Direct2D render target's `PREMULTIPLIED` alpha mode — 32bppBGRA would
    /// produce bright halos around the image.
    pub fn decode_square(&self, bytes: &[u8], size: u32) -> Option<ArtBitmap> {
        unsafe {
            // Does not take ownership: `bytes` must outlive the stream.
            let stream = SHCreateMemStream(Some(bytes))?;
            let decoder = self
                .0
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;

            // Scaling during decode is both cheaper and sharper than letting
            // Direct2D interpolate a full-resolution image down.
            let scaler = self.0.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&frame, size, size, WICBitmapInterpolationModeFant)
                .ok()?;

            let converter = self.0.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &scaler,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeMedianCut,
                )
                .ok()?;

            let stride = size * 4;
            let mut data = vec![0u8; (stride * size) as usize];
            converter
                .CopyPixels(std::ptr::null(), stride, &mut data)
                .ok()?;

            Some(ArtBitmap {
                w: size,
                h: size,
                stride,
                data,
            })
        }
    }
}

/// Builds the tray icon: a small music note drawn straight into a premultiplied
/// BGRA buffer, then wrapped in an HICON. Avoids shipping a .ico resource.
pub fn tray_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

    const S: u32 = 32;
    let mut data = vec![0u8; (S * S * 4) as usize];

    // Eighth note: an elliptical head, a stem, and a flag.
    for y in 0..S {
        for x in 0..S {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let dx = (fx - 11.0) / 7.0;
            let dy = (fy - 23.0) / 5.5;
            let in_head = dx * dx + dy * dy <= 1.0;
            let in_stem = (16.5..19.5).contains(&fx) && (5.0..24.0).contains(&fy);
            let in_flag = (5.0..12.0).contains(&fy) && fx >= 18.5 && fx <= 18.5 + (12.0 - fy) * 1.2;
            if in_head || in_stem || in_flag {
                let i = ((y * S + x) * 4) as usize;
                // Light glyph so it stays legible on both light and dark taskbars.
                data[i] = 240; // B
                data[i + 1] = 240; // G
                data[i + 2] = 240; // R
                data[i + 3] = 255; // A
            }
        }
    }

    unsafe {
        // CreateBitmap returns HBITMAP directly (null on failure), not a Result.
        let color = CreateBitmap(S as i32, S as i32, 1, 32, Some(data.as_ptr() as *const _));
        if color.is_invalid() {
            return None;
        }
        // 1bpp mask, all zero: tells Windows to use the alpha channel instead.
        let mask_bits = [0u8; 4 * S as usize];
        let mask = CreateBitmap(
            S as i32,
            S as i32,
            1,
            1,
            Some(mask_bits.as_ptr() as *const _),
        );

        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();

        let _ = DeleteObject(HGDIOBJ::from(color));
        if !mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ::from(mask));
        }
        icon
    }
}

impl Wic {
    /// Reads embedded art from a track and decodes it. Returns None when the file
    /// has no embedded picture (routine for WAV).
    pub fn art_for_track(&self, path: &std::path::Path, size: u32) -> Option<ArtBitmap> {
        use lofty::file::TaggedFileExt;

        let tagged = lofty::read_from_path(path).ok()?;
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
        let picture = tag.pictures().first()?;
        self.decode_square(picture.data(), size)
    }
}
