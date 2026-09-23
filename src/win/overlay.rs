//! The ring menu: a layered, click-through, never-focused window drawn with Direct2D.
//! Visual state comes from `Anim`; the app calls `tick` every frame while it's animating.

use std::f32::consts::FRAC_PI_4;
use std::mem::size_of;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::anim::{Anim, shortest_turn};
use crate::config::{DEAD_ZONE_PX, action};
use crate::direction::Direction;
use crate::tracker::Point;

// Sizes in logical pixels (scaled by the monitor's DPI).
const OUTER_RADIUS: f32 = 95.0;
const LABEL_RADIUS: f32 = 64.0;
const HIGHLIGHT_RADIUS: f32 = 27.0;
const FONT_SIZE: f32 = 11.0;
/// Room around the disc for the shadow.
const SHADOW: f32 = 8.0;
/// Ring grows from this size to full size as it opens.
const OPEN_FROM_SCALE: f32 = 0.92;

// Grey palette (r, g, b, a).
const DISC_TOP: [f32; 4] = [0.23, 0.23, 0.24, 0.95];
const DISC_BOTTOM: [f32; 4] = [0.13, 0.13, 0.14, 0.95];
const RIM_DARK: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
const RIM_LIGHT: [f32; 4] = [1.0, 1.0, 1.0, 0.12];
const DIVIDER: [f32; 4] = [1.0, 1.0, 1.0, 0.07];
const CENTER_FILL: [f32; 4] = [0.0, 0.0, 0.0, 0.18];
const CENTER_EDGE: [f32; 4] = [1.0, 1.0, 1.0, 0.14];
const BUBBLE: [f32; 4] = [0.38, 0.38, 0.40, 1.0];
const BUBBLE_EDGE: [f32; 4] = [1.0, 1.0, 1.0, 0.22];
const LABEL: [f32; 4] = [0.74, 0.74, 0.76, 1.0];
const LABEL_ACTIVE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SHADOW_LAYER: [f32; 4] = [0.0, 0.0, 0.0, 0.05];

pub struct Overlay {
    hwnd: HWND,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    surface: Option<Surface>,
    origin: Point,
    anim: Option<Anim>,
    last_frame: Instant,
}

/// Off-screen bitmap, render target, and brushes for one DPI scale.
struct Surface {
    scale: f32,
    size: i32,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    target: ID2D1DCRenderTarget,
    text: IDWriteTextFormat,
    brushes: Brushes,
}

struct Brushes {
    disc: ID2D1LinearGradientBrush,
    rim_dark: ID2D1SolidColorBrush,
    rim_light: ID2D1SolidColorBrush,
    divider: ID2D1SolidColorBrush,
    center_fill: ID2D1SolidColorBrush,
    center_edge: ID2D1SolidColorBrush,
    bubble: ID2D1SolidColorBrush,
    bubble_edge: ID2D1SolidColorBrush,
    /// Recolored per label.
    label: ID2D1SolidColorBrush,
    shadow: ID2D1SolidColorBrush,
}

impl Overlay {
    pub fn new(instance: HINSTANCE) -> Result<Self> {
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                lpszClassName: w!("FlickOverlay"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                w!("FlickOverlay"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?;
            Ok(Overlay {
                hwnd,
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                surface: None,
                origin: Point { x: 0, y: 0 },
                anim: None,
                last_frame: Instant::now(),
            })
        }
    }

    pub fn show(&mut self, origin: Point, highlight: Option<Direction>) -> Result<()> {
        let scale = dpi_scale(origin);
        if self.surface.as_ref().is_none_or(|s| s.scale != scale) {
            self.surface = None;
            self.surface = Some(Surface::new(&self.d2d, &self.dwrite, scale)?);
        }
        self.origin = origin;
        self.anim = Some(Anim::opening(highlight));
        self.last_frame = Instant::now();
        self.render()?;
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }

    pub fn set_highlight(&mut self, highlight: Option<Direction>) {
        if let Some(anim) = &mut self.anim {
            anim.set_highlight(highlight);
        }
    }

    /// Starts the fade-out; the window hides itself once it finishes.
    pub fn hide(&mut self) {
        if let Some(anim) = &mut self.anim {
            anim.close();
        }
    }

    /// True while the ring is on screen (including fading out).
    pub fn is_active(&self) -> bool {
        self.anim.is_some()
    }

    /// Advances one frame. Returns true while there is still animation to run.
    pub fn tick(&mut self) -> Result<bool> {
        let Some(anim) = &mut self.anim else {
            return Ok(false);
        };
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;
        anim.step(dt);
        if anim.closed() {
            self.anim = None;
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            return Ok(false);
        }
        let settled = anim.settled();
        self.render()?;
        Ok(!settled)
    }

    fn render(&self) -> Result<()> {
        let (Some(s), Some(anim)) = (&self.surface, &self.anim) else {
            return Ok(());
        };
        let b = &s.brushes;
        let (k, size) = (s.scale, s.size);
        let c = size as f32 / 2.0;
        let at = |angle: f32, radius: f32| Vector2 {
            X: c + radius * angle.cos(),
            Y: c - radius * angle.sin(),
        };
        unsafe {
            let t = &s.target;
            t.BindDC(
                s.dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: size,
                    bottom: size,
                },
            )?;
            t.BeginDraw();
            t.Clear(Some(&rgba([0.0; 4])));
            let grow = OPEN_FROM_SCALE + (1.0 - OPEN_FROM_SCALE) * anim.open;
            t.SetTransform(&Matrix3x2::scale_around(grow, grow, Vector2 { X: c, Y: c }));

            // Soft shadow: stacked faint circles, offset slightly down.
            for i in 1..=4 {
                let r = (OUTER_RADIUS + i as f32 * SHADOW / 4.0) * k;
                t.FillEllipse(&circle(c, c + 2.0 * k, r), &b.shadow);
            }

            let outer = OUTER_RADIUS * k;
            t.FillEllipse(&circle(c, c, outer), &b.disc);

            // Faint dividers between the eight slices.
            let dead_zone = DEAD_ZONE_PX as f32;
            for i in 0..8 {
                let a = (i as f32 + 0.5) * FRAC_PI_4;
                t.DrawLine(
                    at(a, dead_zone + 8.0 * k),
                    at(a, outer - 10.0 * k),
                    &b.divider,
                    k,
                    None,
                );
            }

            // Releasing inside this ring cancels.
            let center = circle(c, c, dead_zone);
            t.FillEllipse(&center, &b.center_fill);
            t.DrawEllipse(&center, &b.center_edge, k, None);

            // The bubble glides between slices and fades at the center.
            let p = at(anim.angle, LABEL_RADIUS * k);
            let bubble = circle(p.X, p.Y, HIGHLIGHT_RADIUS * k);
            b.bubble.SetOpacity(anim.glow);
            b.bubble_edge.SetOpacity(anim.glow);
            t.FillEllipse(&bubble, &b.bubble);
            t.DrawEllipse(&bubble, &b.bubble_edge, k, None);

            for dir in Direction::ALL {
                // Labels brighten as the bubble passes over them.
                let near = 1.0 - (shortest_turn(anim.angle, dir.angle()).abs() / FRAC_PI_4);
                let weight = anim.glow * near.max(0.0);
                b.label.SetColor(&rgba(mix(LABEL, LABEL_ACTIVE, weight)));
                let p = at(dir.angle(), LABEL_RADIUS * k);
                let label: Vec<u16> = action(dir).label.encode_utf16().collect();
                let rect = D2D_RECT_F {
                    left: p.X - 32.0 * k,
                    top: p.Y - 16.0 * k,
                    right: p.X + 32.0 * k,
                    bottom: p.Y + 16.0 * k,
                };
                t.DrawText(
                    &label,
                    &s.text,
                    &rect,
                    &b.label,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // Bevelled rim: dark outer line, light hairline just inside.
            t.DrawEllipse(&circle(c, c, outer), &b.rim_dark, 1.5 * k, None);
            t.DrawEllipse(&circle(c, c, outer - 1.5 * k), &b.rim_light, k, None);

            t.EndDraw(None, None)?;

            let dst = POINT {
                x: self.origin.x - size / 2,
                y: self.origin.y - size / 2,
            };
            let extent = SIZE { cx: size, cy: size };
            let src = POINT::default();
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                // Whole-window fade for open/close.
                SourceConstantAlpha: (anim.open * 255.0).round() as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&dst),
                Some(&extent),
                Some(s.dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }
}

impl Surface {
    fn new(d2d: &ID2D1Factory, dwrite: &IDWriteFactory, scale: f32) -> Result<Self> {
        let size = ((OUTER_RADIUS + SHADOW + 4.0) * 2.0 * scale).ceil() as i32;
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let old = SelectObject(dc, bitmap.into());

            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                // 96 DPI so Direct2D units are physical pixels; we scale ourselves.
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = d2d.CreateDCRenderTarget(&props)?;

            let text = dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                FONT_SIZE * scale,
                w!("en-us"),
            )?;
            text.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

            let brushes = Brushes::new(&target, size as f32 / 2.0, OUTER_RADIUS * scale)?;

            Ok(Surface {
                scale,
                size,
                dc,
                bitmap,
                old,
                target,
                text,
                brushes,
            })
        }
    }
}

impl Brushes {
    /// `c` is the disc center and `outer` its radius, both in pixels.
    fn new(t: &ID2D1DCRenderTarget, c: f32, outer: f32) -> Result<Self> {
        unsafe {
            let solid = |color| t.CreateSolidColorBrush(&rgba(color), None);
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: rgba(DISC_TOP),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: rgba(DISC_BOTTOM),
                },
            ];
            let stops =
                t.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let disc = t.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: c, Y: c - outer },
                    endPoint: Vector2 { X: c, Y: c + outer },
                },
                None,
                &stops,
            )?;
            Ok(Brushes {
                disc,
                rim_dark: solid(RIM_DARK)?,
                rim_light: solid(RIM_LIGHT)?,
                divider: solid(DIVIDER)?,
                center_fill: solid(CENTER_FILL)?,
                center_edge: solid(CENTER_EDGE)?,
                bubble: solid(BUBBLE)?,
                bubble_edge: solid(BUBBLE_EDGE)?,
                label: solid(LABEL)?,
                shadow: solid(SHADOW_LAYER)?,
            })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn dpi_scale(p: Point) -> f32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: p.x, y: p.y }, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x as f32 / 96.0
    }
}

fn rgba([r, g, b, a]: [f32; 4]) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn mix(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
}

fn circle(x: f32, y: f32, radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2 { X: x, Y: y },
        radiusX: radius,
        radiusY: radius,
    }
}
