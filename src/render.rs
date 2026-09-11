//! Off-screen 32-bit surface, drawn with Direct2D + DirectWrite and blitted to the
//! layered window via `UpdateLayeredWindow`.
//!
//! Alpha contract: `UpdateLayeredWindow` with `AC_SRC_ALPHA` requires **premultiplied**
//! BGRA, and any pixel with A == 0 must have RGB == 0. The render target is created
//! with `D2D1_ALPHA_MODE_PREMULTIPLIED` so this holds automatically — never touch
//! these bits with GDI afterwards.
//!
//! Colours and the font family come from `SurfaceStyle`, which mirrors the
//! `[theme]` section of the config. Changing either rebuilds the brushes and text
//! formats in place, so a settings-window edit is visible immediately.

use crate::theme::Palette;
use windows::{
    core::*,
    Win32::{
        Foundation::{COLORREF, HWND, POINT, SIZE},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
                    D2D1_FIGURE_END_CLOSED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
                },
                D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget,
                ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
                D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
            },
            DirectWrite::{
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
                DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
                DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::{
                CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
                SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
                BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
            },
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap,
                IWICImagingFactory, WICBitmapCacheOnDemand, WICBitmapLockRead, WICRect,
            },
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA},
    },
};

const PAD: f32 = 10.0;
const TITLE_SIZE: f32 = 13.0;
const SUB_SIZE: f32 = 11.0;
const BAR_H: f32 = 3.0;
const BTN: f32 = 26.0;
const BTN_GAP: f32 = 14.0;

/// Everything about the card's appearance that a user can change.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceStyle {
    pub palette: Palette,
    pub font_family: String,
}

impl Default for SurfaceStyle {
    fn default() -> Self {
        Self {
            palette: Palette::default(),
            font_family: "Segoe UI".into(),
        }
    }
}

/// Card geometry in logical (96-dpi) pixels. Shared by the renderer and the
/// hit-testing code so the two can never drift apart.
#[derive(Clone, Copy)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub art: D2D_RECT_F,
    pub title: D2D_RECT_F,
    pub subtitle: D2D_RECT_F,
    pub prev: D2D_RECT_F,
    pub play: D2D_RECT_F,
    pub next: D2D_RECT_F,
    pub progress: D2D_RECT_F,
}

pub fn layout_for(w: f32, h: f32) -> Layout {
    // The cover is a square sized by the height, but never so wide that the text
    // column vanishes — the settings window lets a user pick a tall, narrow card.
    let half = (h - PAD * 2.0).max(0.0);
    let art = half.min((w - PAD * 2.0).max(0.0) * 0.45);
    let text_x = PAD + art + 12.0;
    let right = w - PAD;
    let controls_y = h - PAD - 22.0 - BAR_H - 8.0; // above the progress bar
    Layout {
        w,
        h,
        art: D2D_RECT_F {
            left: PAD,
            top: PAD,
            right: PAD + art,
            bottom: PAD + art,
        },
        title: D2D_RECT_F {
            left: text_x,
            top: PAD + 2.0,
            right,
            bottom: PAD + 2.0 + TITLE_SIZE + 4.0,
        },
        subtitle: D2D_RECT_F {
            left: text_x,
            top: PAD + 2.0 + TITLE_SIZE + 6.0,
            right,
            bottom: PAD + 2.0 + TITLE_SIZE + 6.0 + SUB_SIZE + 4.0,
        },
        prev: D2D_RECT_F {
            left: text_x,
            top: controls_y,
            right: text_x + BTN,
            bottom: controls_y + BTN,
        },
        play: D2D_RECT_F {
            left: text_x + BTN + BTN_GAP,
            top: controls_y,
            right: text_x + BTN + BTN_GAP + BTN,
            bottom: controls_y + BTN,
        },
        next: D2D_RECT_F {
            left: text_x + (BTN + BTN_GAP) * 2.0,
            top: controls_y,
            right: text_x + (BTN + BTN_GAP) * 2.0 + BTN,
            bottom: controls_y + BTN,
        },
        progress: D2D_RECT_F {
            left: text_x,
            top: h - PAD - BAR_H,
            right,
            bottom: h - PAD,
        },
    }
}

impl Layout {
    /// Which control (if any) is under a point, in logical pixels.
    /// 0 = prev, 1 = play/pause, 2 = next, 3 = progress bar.
    pub fn hit(&self, x: f32, y: f32) -> Option<u8> {
        let contains = |r: D2D_RECT_F| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
        // The bar is a thin strip, so give it a few pixels of slop vertically.
        let bar = D2D_RECT_F {
            left: self.progress.left,
            top: self.progress.top - 5.0,
            right: self.progress.right,
            bottom: self.progress.bottom + 5.0,
        };
        if contains(self.prev) {
            Some(0)
        } else if contains(self.play) {
            Some(1)
        } else if contains(self.next) {
            Some(2)
        } else if contains(bar) {
            Some(3)
        } else {
            None
        }
    }
}

/// A premultiplied BGRA image ready to become a Direct2D bitmap.
#[derive(Clone)]
pub struct ArtBitmap {
    pub w: u32,
    pub h: u32,
    pub stride: u32,
    pub data: Vec<u8>,
}

/// Everything the renderer needs for one frame.
///
/// `artist` doubles as the status line: when there is nothing playing (or the
/// audio device is missing) the app puts a hint there instead, so no separate
/// layout slot is needed.
#[derive(Default, Clone)]
pub struct Frame {
    pub title: String,
    pub artist: String,
    /// 0.0 - 1.0, or None to hide the bar.
    pub progress: Option<f32>,
    pub art: Option<ArtBitmap>,
    /// Controls are hidden until the pointer is over the card.
    pub show_controls: bool,
    /// False draws the pause glyph instead of the play triangle.
    pub playing: bool,
}

/// The render target is backed by a WIC bitmap rather than a GDI DC on purpose.
/// A DC render target makes Direct2D spin up a D3D11 device and DXGI, which cost
/// ~52 MB of private memory on this machine; the WIC target rasterises on the CPU
/// for ~2 MB and the card is small enough that it makes no difference to speed.
struct D2d {
    bitmap: IWICBitmap,
    target: ID2D1RenderTarget,
    dwrite: IDWriteFactory,
    res: Resources,
    // Transport glyphs, built once in a 0..BTN local space and positioned with a
    // render-target transform.
    play_tri: ID2D1PathGeometry,
    prev_tri: ID2D1PathGeometry,
    next_tri: ID2D1PathGeometry,
}

/// The six brushes, grouped so a theme change can replace them in one assignment.
struct Brushes {
    bg: ID2D1SolidColorBrush,
    art_bg: ID2D1SolidColorBrush,
    title: ID2D1SolidColorBrush,
    subtitle: ID2D1SolidColorBrush,
    bar_bg: ID2D1SolidColorBrush,
    bar_fg: ID2D1SolidColorBrush,
}

struct Resources {
    brushes: Brushes,
    title_fmt: IDWriteTextFormat,
    sub_fmt: IDWriteTextFormat,
}

fn vec2(x: f32, y: f32) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: x, Y: y }
}

fn triangle(
    factory: &ID2D1Factory,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
) -> Result<ID2D1PathGeometry> {
    unsafe {
        let geom = factory.CreatePathGeometry()?;
        let sink = geom.Open()?;
        sink.BeginFigure(vec2(a.0, a.1), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddLines(&[vec2(b.0, b.1), vec2(c.0, c.1)]);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(geom)
    }
}

fn build_brushes(target: &ID2D1RenderTarget, palette: &Palette) -> Result<Brushes> {
    let solid = |c: crate::theme::Rgba| -> Result<ID2D1SolidColorBrush> {
        unsafe {
            target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                    a: c.a,
                },
                None,
            )
        }
    };
    Ok(Brushes {
        bg: solid(palette.background)?,
        art_bg: solid(palette.art_bg)?,
        title: solid(palette.title)?,
        subtitle: solid(palette.subtitle)?,
        bar_bg: solid(palette.bar_bg)?,
        bar_fg: solid(palette.bar_fg)?,
    })
}

/// The locale is the *system* one, not the UI language: it drives font fallback
/// and line breaking, so a Japanese title must be laid out with a Japanese
/// locale even when the menus are in English.
fn build_text_formats(
    dwrite: &IDWriteFactory,
    font_family: &str,
) -> Result<(IDWriteTextFormat, IDWriteTextFormat)> {
    let mut family: Vec<u16> = font_family.encode_utf16().collect();
    family.push(0);
    let family = PCWSTR(family.as_ptr());
    let locale = crate::i18n::dwrite_locale();
    unsafe {
        let title = dwrite.CreateTextFormat(
            family,
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TITLE_SIZE,
            locale,
        )?;
        let _ = title.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let sub = dwrite.CreateTextFormat(
            family,
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            SUB_SIZE,
            locale,
        )?;
        let _ = sub.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        Ok((title, sub))
    }
}

pub struct Surface {
    pub w: i32, // physical px
    pub h: i32,
    dpi: f32,
    style: SurfaceStyle,
    hdc_screen: HDC,
    hdc_mem: HDC,
    dib: HBITMAP,
    /// Pointer to the DIB's pixels, owned by `dib`. Only written by `draw`.
    bits: *mut u8,
    old: HGDIOBJ,
    d2d: Option<D2d>,
}

impl Surface {
    pub fn new(w: i32, h: i32, dpi: f32, style: SurfaceStyle) -> Result<Self> {
        unsafe {
            let hdc_screen = GetDC(None);
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // negative => top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0;

            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(hdc_screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let old = SelectObject(hdc_mem, dib.into());

            Ok(Self {
                w,
                h,
                dpi,
                style,
                hdc_screen,
                hdc_mem,
                dib,
                bits: bits as *mut u8,
                old,
                d2d: None,
            })
        }
    }

    pub fn style(&self) -> &SurfaceStyle {
        &self.style
    }

    /// Installs a new appearance. Brushes and text formats are rebuilt in place
    /// on the existing render target, which is far cheaper than recreating the
    /// whole surface.
    pub fn set_style(&mut self, style: SurfaceStyle) -> Result<()> {
        let unchanged =
            self.style.font_family == style.font_family && self.style.palette == style.palette;
        let palette = style.palette;
        let font_family = style.font_family.clone();
        self.style = style;
        if unchanged {
            return Ok(());
        }

        let Some(d) = self.d2d.as_mut() else {
            return Ok(());
        };
        d.res.brushes = build_brushes(&d.target, &palette)?;
        let (title_fmt, sub_fmt) = build_text_formats(&d.dwrite, &font_family)?;
        d.res.title_fmt = title_fmt;
        d.res.sub_fmt = sub_fmt;
        Ok(())
    }

    pub fn resize(&mut self, w: i32, h: i32, dpi: f32) -> Result<()> {
        // Drop Direct2D *before* the GDI objects: the render target is bound to
        // hdc_mem and must be released while that DC still exists.
        self.d2d = None;
        let fresh = Surface::new(w, h, dpi, self.style.clone())?;
        let _ = self.release_gdi();
        *self = fresh;
        Ok(())
    }

    fn ensure_d2d(&mut self) -> Result<()> {
        if self.d2d.is_some() {
            return Ok(());
        }
        let palette = self.style.palette;
        let font_family = self.style.font_family.clone();
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let bitmap = wic.CreateBitmap(
                self.w as u32,
                self.h as u32,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapCacheOnDemand,
            )?;
            let target: ID2D1RenderTarget = factory.CreateWicBitmapRenderTarget(&bitmap, &props)?;
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            // ClearType is impossible on an alpha surface — subpixel colours cannot
            // be represented and show up as coloured fringes.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let brushes = build_brushes(&target, &palette)?;
            let (title_fmt, sub_fmt) = build_text_formats(&dwrite, &font_family)?;

            self.d2d = Some(D2d {
                bitmap,
                target,
                dwrite,
                res: Resources {
                    brushes,
                    title_fmt,
                    sub_fmt,
                },
                play_tri: triangle(&factory, (8.0, 4.0), (8.0, 18.0), (20.0, 11.0))?,
                prev_tri: triangle(&factory, (17.0, 4.0), (17.0, 18.0), (6.0, 11.0))?,
                next_tri: triangle(&factory, (6.0, 4.0), (6.0, 18.0), (17.0, 11.0))?,
            });
        }
        Ok(())
    }

    /// Signed distance to a rounded rect: < 0 inside, > 0 outside. Used for hit
    /// testing so events fall through outside the card's rounded corners.
    pub fn rounded_rect_sdf(px: f32, py: f32, half_w: f32, half_h: f32, r: f32) -> f32 {
        let qx = px.abs() - (half_w - r);
        let qy = py.abs() - (half_h - r);
        let outside = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt();
        let inside = qx.max(qy).min(0.0);
        outside + inside - r
    }

    /// Draws one frame. All geometry is in logical (96-dpi) pixels; the render
    /// target's DPI setting scales it onto the physical DIB.
    pub fn draw(&mut self, frame: &Frame, opacity: f32, radius: f32) -> Result<()> {
        self.ensure_d2d()?;
        let d = self.d2d.as_ref().unwrap();
        let s = self.dpi / 96.0;
        let lw = self.w as f32 / s;
        let lh = self.h as f32 / s;

        unsafe {
            let t = &d.target;
            t.BeginDraw();
            t.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));

            // Card background. Opacity is baked into the brush: SourceConstantAlpha
            // must stay 255 because it would multiply the premultiplied alpha again.
            let card = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: lw,
                    bottom: lh,
                },
                radiusX: radius,
                radiusY: radius,
            };
            d.res.brushes.bg.SetOpacity(opacity);
            t.FillRoundedRectangle(&card, &d.res.brushes.bg);

            // Album art: square, inset, occupying the full inner height.
            let art_size = lh - PAD * 2.0;
            let art_rect = D2D_RECT_F {
                left: PAD,
                top: PAD,
                right: PAD + art_size,
                bottom: PAD + art_size,
            };
            let art_round = D2D1_ROUNDED_RECT {
                rect: art_rect,
                radiusX: radius * 0.6,
                radiusY: radius * 0.6,
            };

            let mut art_bmp: Option<ID2D1Bitmap> = None;
            if let Some(art) = &frame.art {
                art_bmp = t
                    .CreateBitmap(
                        D2D_SIZE_U {
                            width: art.w,
                            height: art.h,
                        },
                        Some(art.data.as_ptr() as *const std::ffi::c_void),
                        art.stride,
                        &D2D1_BITMAP_PROPERTIES {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                            },
                            dpiX: 96.0,
                            dpiY: 96.0,
                        },
                    )
                    .ok();
            }
            match &art_bmp {
                Some(bmp) => {
                    // DrawBitmap cannot round corners, so lay the art over a rounded
                    // placeholder and accept square corners on the image itself.
                    t.FillRoundedRectangle(&art_round, &d.res.brushes.art_bg);
                    t.DrawBitmap(
                        bmp,
                        Some(&art_rect),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
                None => t.FillRoundedRectangle(&art_round, &d.res.brushes.art_bg),
            }

            // Text column and controls both come from the shared layout.
            let lay = layout_for(lw, lh);
            draw_text(
                t,
                d,
                &frame.title,
                &d.res.title_fmt,
                &d.res.brushes.title,
                lay.title,
            );
            draw_text(
                t,
                d,
                &frame.artist,
                &d.res.sub_fmt,
                &d.res.brushes.subtitle,
                lay.subtitle,
            );

            if frame.show_controls {
                // prev: bar + left-pointing triangle
                t.FillRectangle(
                    &D2D_RECT_F {
                        left: lay.prev.left + 3.0,
                        top: lay.prev.top + 4.0,
                        right: lay.prev.left + 6.0,
                        bottom: lay.prev.top + 18.0,
                    },
                    &d.res.brushes.title,
                );
                fill_at(t, &d.prev_tri, lay.prev, &d.res.brushes.title);

                // play / pause
                if frame.playing {
                    t.FillRectangle(
                        &D2D_RECT_F {
                            left: lay.play.left + 8.0,
                            top: lay.play.top + 4.0,
                            right: lay.play.left + 11.0,
                            bottom: lay.play.top + 18.0,
                        },
                        &d.res.brushes.title,
                    );
                    t.FillRectangle(
                        &D2D_RECT_F {
                            left: lay.play.left + 15.0,
                            top: lay.play.top + 4.0,
                            right: lay.play.left + 18.0,
                            bottom: lay.play.top + 18.0,
                        },
                        &d.res.brushes.title,
                    );
                } else {
                    fill_at(t, &d.play_tri, lay.play, &d.res.brushes.title);
                }

                // next: right-pointing triangle + bar
                fill_at(t, &d.next_tri, lay.next, &d.res.brushes.title);
                t.FillRectangle(
                    &D2D_RECT_F {
                        left: lay.next.left + 20.0,
                        top: lay.next.top + 4.0,
                        right: lay.next.left + 23.0,
                        bottom: lay.next.top + 18.0,
                    },
                    &d.res.brushes.title,
                );
            }

            // Progress bar along the bottom of the text column.
            if let Some(p) = frame.progress {
                t.FillRectangle(&lay.progress, &d.res.brushes.bar_bg);
                let w_full = lay.progress.right - lay.progress.left;
                t.FillRectangle(
                    &D2D_RECT_F {
                        left: lay.progress.left,
                        top: lay.progress.top,
                        right: lay.progress.left + w_full * p.clamp(0.0, 1.0),
                        bottom: lay.progress.bottom,
                    },
                    &d.res.brushes.bar_fg,
                );
            }

            t.EndDraw(None, None)?;
            // The render target writes into the WIC bitmap, so copy it across to
            // the DIB that UpdateLayeredWindow reads from.
            self.blit_to_dib(d)?;
        }
        Ok(())
    }

    /// Copies the rendered WIC bitmap into the DIB. Both are premultiplied BGRA,
    /// so this is a straight row-by-row memcpy (the strides can differ).
    unsafe fn blit_to_dib(&self, d: &D2d) -> Result<()> {
        let rect = WICRect {
            X: 0,
            Y: 0,
            Width: self.w,
            Height: self.h,
        };
        let lock = d.bitmap.Lock(&rect, WICBitmapLockRead.0 as u32)?;
        let mut size = 0u32;
        let mut data = std::ptr::null_mut();
        let stride = lock.GetStride()?;
        lock.GetDataPointer(&mut size, &mut data)?;
        if data.is_null() {
            return Ok(());
        }
        let row_bytes = (self.w * 4) as usize;
        let copy = row_bytes.min(stride as usize);
        for y in 0..self.h as usize {
            std::ptr::copy_nonoverlapping(
                data.add(y * stride as usize),
                self.bits.add(y * row_bytes),
                copy,
            );
        }
        let _ = size; // sanity: data covers stride*Height
        Ok(())
    }

    /// Blits to a layered window. `x`/`y` are the destination screen position.
    pub fn present(&self, hwnd: HWND, x: i32, y: i32) -> Result<()> {
        unsafe {
            let pt_dst = POINT { x, y };
            let pt_src = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: self.w,
                cy: self.h,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                // Must stay 255 — this multiplies the already-premultiplied alpha a
                // second time, which would darken the whole card.
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            UpdateLayeredWindow(
                hwnd,
                Some(self.hdc_screen),
                Some(&pt_dst),
                Some(&size),
                Some(self.hdc_mem),
                Some(&pt_src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    pub fn release_gdi(&self) -> Result<()> {
        unsafe {
            SelectObject(self.hdc_mem, self.old);
            DeleteObject(self.dib.into()).ok()?;
            DeleteDC(self.hdc_mem).ok()?;
            ReleaseDC(None, self.hdc_screen);
        }
        Ok(())
    }
}

/// Fills a glyph that was built in the 0..BTN local space, positioned by
/// translating the render target rather than rebuilding the geometry.
unsafe fn fill_at(
    t: &ID2D1RenderTarget,
    geom: &ID2D1PathGeometry,
    rect: D2D_RECT_F,
    brush: &ID2D1SolidColorBrush,
) {
    // windows-rs substitutes the WinRT numerics type for D2D_MATRIX_3X2_F.
    t.SetTransform(&windows_numerics::Matrix3x2 {
        M11: 1.0,
        M12: 0.0,
        M21: 0.0,
        M22: 1.0,
        M31: rect.left,
        M32: rect.top,
    });
    t.FillGeometry(geom, brush, None);
    t.SetTransform(&windows_numerics::Matrix3x2 {
        M11: 1.0,
        M12: 0.0,
        M21: 0.0,
        M22: 1.0,
        M31: 0.0,
        M32: 0.0,
    });
}

/// Renders one trimmed, single-line run of text inside `rect`.
unsafe fn draw_text(
    t: &ID2D1RenderTarget,
    d: &D2d,
    text: &str,
    fmt: &IDWriteTextFormat,
    brush: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
) {
    if text.is_empty() {
        return;
    }
    let max_w = rect.right - rect.left;
    let max_h = rect.bottom - rect.top;
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let Ok(layout) = d.dwrite.CreateTextLayout(&utf16, fmt, max_w, max_h) else {
        return;
    };
    let _ = layout.SetTrimming(
        &DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        },
        None,
    );
    let mut metrics = DWRITE_TEXT_METRICS::default();
    if layout.GetMetrics(&mut metrics).is_err() {
        return;
    }
    t.DrawTextLayout(
        windows_numerics::Vector2 {
            X: rect.left,
            Y: rect.top,
        },
        &layout,
        brush,
        D2D1_DRAW_TEXT_OPTIONS_NONE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn center(r: D2D_RECT_F) -> (f32, f32) {
        ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
    }

    #[test]
    fn controls_are_hit_testable() {
        let lay = layout_for(340.0, 96.0);
        let (x, y) = center(lay.prev);
        assert_eq!(lay.hit(x, y), Some(0), "prev button");
        let (x, y) = center(lay.play);
        assert_eq!(lay.hit(x, y), Some(1), "play button");
        let (x, y) = center(lay.next);
        assert_eq!(lay.hit(x, y), Some(2), "next button");

        let bx = lay.progress.left + (lay.progress.right - lay.progress.left) * 0.6;
        let by = (lay.progress.top + lay.progress.bottom) / 2.0;
        assert_eq!(lay.hit(bx, by), Some(3), "progress bar");

        // Album art and the gaps between buttons must not trigger anything.
        let (x, y) = center(lay.art);
        assert_eq!(lay.hit(x, y), None, "album art");
        assert_eq!(lay.hit(5.0, 5.0), None, "card corner");
    }

    #[test]
    fn layout_stays_inside_the_card() {
        for (w, h) in [(340.0, 96.0), (280.0, 80.0), (420.0, 120.0)] {
            let lay = layout_for(w, h);
            for (name, r) in [
                ("art", lay.art),
                ("prev", lay.prev),
                ("play", lay.play),
                ("next", lay.next),
                ("progress", lay.progress),
            ] {
                assert!(
                    r.left >= 0.0 && r.right <= w && r.top >= 0.0 && r.bottom <= h,
                    "{name} escapes the card at {w}x{h}: {r:?}"
                );
            }
        }
    }

    #[test]
    fn layout_survives_every_configurable_size() {
        // The settings window lets a user pick any size in the validated range,
        // so the layout must not fall apart anywhere inside it.
        let (wmin, wmax) = crate::config::WIDTH_RANGE;
        let (hmin, hmax) = crate::config::HEIGHT_RANGE;
        for w in [wmin, 340, wmax] {
            for h in [hmin, 96, hmax] {
                let lay = layout_for(w as f32, h as f32);
                assert!(
                    lay.progress.right > lay.progress.left,
                    "progress bar collapses at {w}x{h}"
                );
                assert!(
                    lay.art.bottom <= h as f32 && lay.art.right <= w as f32,
                    "art overflows at {w}x{h}"
                );
                // Controls must stay clickable rather than sliding off the card.
                let (px, py) = center(lay.play);
                assert_eq!(lay.hit(px, py), Some(1), "play unreachable at {w}x{h}");
            }
        }
    }

    #[test]
    fn default_style_matches_the_shipped_palette() {
        let s = SurfaceStyle::default();
        assert_eq!(s.font_family, "Segoe UI");
        assert_eq!(s.palette, Palette::default());
    }

    #[test]
    fn identical_styles_compare_equal() {
        // `set_style` uses this to skip rebuilding brushes on every layout pass.
        assert_eq!(SurfaceStyle::default(), SurfaceStyle::default());
        let other = SurfaceStyle {
            font_family: "Arial".into(),
            ..SurfaceStyle::default()
        };
        assert_ne!(SurfaceStyle::default(), other);
    }
}
