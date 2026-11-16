use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Once;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_BOLD,
    HDC, TRANSPARENT,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipDisposeImage, GdipDrawImageRectI,
    GdipGetImageHeight, GdipGetImageWidth, GdipLoadImageFromFile,
    GdiplusStartup, GdiplusStartupInput, GpImage, Status,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, KillTimer, RegisterClassW, SetTimer,
    SetWindowPos, ShowWindow, UpdateLayeredWindow, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

const HUD_TIMER_ID: usize = 1;
const HUD_CLASS_NAME: PCWSTR = w!("WintilHudWindowClass");

const DEFAULT_HUD_WIDTH: i32 = 160;
const DEFAULT_HUD_HEIGHT: i32 = 36;

static HUD_HWND_VAL: AtomicIsize = AtomicIsize::new(0);
static GDI_PLUS_INIT: Once = Once::new();

fn ensure_gdiplus_initialized() {
    GDI_PLUS_INIT.call_once(|| unsafe {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let mut token: usize = 0;
        let _ = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
    });
}

unsafe extern "system" fn hud_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            if wparam.0 == HUD_TIMER_ID {
                let _ = KillTimer(hwnd, HUD_TIMER_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[inline]
fn clamp(val: f32, min: f32, max: f32) -> f32 {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

/// Renders a minimalist rectangular badge with declarative colors and text
unsafe fn render_badge(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    is_pinned: bool,
    hdc_mem: HDC,
    config: &crate::config::PinHudConfig,
    scale: &crate::flyout::DpiScale,
) {
    let w_f = width as f32;
    let h_f = height as f32;
    let r = scale.scale(8) as f32;

    // 1. Text rasterization pass: render text directly into DIB via DrawTextW
    pixels.fill(0);

    let font_height = scale.scale(18);
    let font = CreateFontW(
        font_height,
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Segoe UI"),
    );
    let old_font = SelectObject(hdc_mem, font);

    SetBkMode(hdc_mem, TRANSPARENT);
    SetTextColor(hdc_mem, COLORREF(0x00FFFFFF)); // White glyph mask

    let label_str = if is_pinned {
        &config.text_pinned
    } else {
        &config.text_unpinned
    };
    let mut wide_label: Vec<u16> = label_str.encode_utf16().chain(std::iter::once(0)).collect();

    let mut text_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

    let len = wide_label.len();
    let _ = DrawTextW(
        hdc_mem,
        &mut wide_label[..len - 1],
        &mut text_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );

    let _ = SelectObject(hdc_mem, old_font);
    let _ = DeleteObject(font);

    // 2. Extract anti-aliased font luminance mask
    let mut text_mask = vec![0u8; (width * height) as usize];
    for (i, &p) in pixels.iter().enumerate() {
        let red = ((p >> 16) & 0xFF) as u8;
        let green = ((p >> 8) & 0xFF) as u8;
        let blue = (p & 0xFF) as u8;
        text_mask[i] = red.max(green).max(blue);
    }

    // 3. Compute smooth black stroke outline by dilating the font mask
    let stroke_radius = scale.scale(2).max(1);
    let max_d_sq = stroke_radius * stroke_radius;
    let mut stroke_mask = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut max_stroke = 0u8;
            for dy in -stroke_radius..=stroke_radius {
                for dx in -stroke_radius..=stroke_radius {
                    let d_sq = dx * dx + dy * dy;
                    if d_sq > max_d_sq {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx >= 0 && nx < width && ny >= 0 && ny < height {
                        let t = text_mask[(ny * width + nx) as usize];
                        if t > 0 {
                            let dist = (d_sq as f32).sqrt();
                            let falloff = (255.0 * (1.0 - (dist / (stroke_radius as f32 + 0.5)) * 0.5)).clamp(100.0, 255.0) as u32;
                            let s = ((t as u32 * falloff) / 255) as u8;
                            if s > max_stroke {
                                max_stroke = s;
                            }
                        }
                    }
                }
            }
            stroke_mask[(y * width + x) as usize] = max_stroke;
        }
    }

    // 4. Declarative Palette Parsing
    let (bg_r, bg_g, bg_b) = crate::config::parse_color_rgb(&config.bg_color, (58.0, 64.0, 74.0));
    let border_r = (bg_r + 36.0).min(255.0);
    let border_g = (bg_g + 38.0).min(255.0);
    let border_b = (bg_b + 42.0).min(255.0);

    let (txt_r, txt_g, txt_b) = crate::config::parse_color_rgb(&config.text_color, (255.0, 255.0, 255.0));
    let (stk_r, stk_g, stk_b) = crate::config::parse_color_rgb(&config.stroke_color, (0.0, 0.0, 0.0));
    let base_alpha = config.opacity.clamp(0.1, 1.0) * 255.0;

    for y in 0..height {
        let py = y as f32 + 0.5;
        let cy = clamp(py, r, h_f - r);

        for x in 0..width {
            let px = x as f32 + 0.5;
            let cx = clamp(px, r, w_f - r);

            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt() - r;

            // Subpixel coverage: -0.5 inside, +0.5 outside
            let edge_coverage = if dist > 0.5 {
                0.0f32
            } else if dist < -0.5 {
                1.0f32
            } else {
                0.5 - dist
            };

            let idx = (y * width + x) as usize;
            if edge_coverage <= 0.001 {
                pixels[idx] = 0;
                continue;
            }

            // 1px border highlight factor
            let border_factor = if dist > -1.2 {
                ((dist + 1.2) / 1.2).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let mut final_r = bg_r * (1.0 - border_factor) + border_r * border_factor;
            let mut final_g = bg_g * (1.0 - border_factor) + border_g * border_factor;
            let mut final_b = bg_b * (1.0 - border_factor) + border_b * border_factor;

            let s_alpha = stroke_mask[idx] as f32 / 255.0;
            let t_alpha = text_mask[idx] as f32 / 255.0;

            let mut card_alpha = base_alpha;

            // Layer 1: Text outline / stroke
            if s_alpha > 0.001 {
                final_r = final_r * (1.0 - s_alpha) + stk_r * s_alpha;
                final_g = final_g * (1.0 - s_alpha) + stk_g * s_alpha;
                final_b = final_b * (1.0 - s_alpha) + stk_b * s_alpha;
                card_alpha = card_alpha * (1.0 - s_alpha) + 245.0 * s_alpha;
            }

            // Layer 2: Main font text
            if t_alpha > 0.001 {
                final_r = final_r * (1.0 - t_alpha) + txt_r * t_alpha;
                final_g = final_g * (1.0 - t_alpha) + txt_g * t_alpha;
                final_b = final_b * (1.0 - t_alpha) + txt_b * t_alpha;
                card_alpha = card_alpha * (1.0 - t_alpha) + 255.0 * t_alpha;
            }

            let a = (edge_coverage * card_alpha).clamp(0.0, 255.0);

            // Premultiplied alpha for Win32 UpdateLayeredWindow
            let a_factor = a / 255.0;
            let pr = (final_r * a_factor).clamp(0.0, 255.0) as u32;
            let pg = (final_g * a_factor).clamp(0.0, 255.0) as u32;
            let pb = (final_b * a_factor).clamp(0.0, 255.0) as u32;
            let pa = a as u32;

            pixels[idx] = (pa << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }
}

/// Attempts to load and render an external custom image (PNG/BMP/etc.) via GDI+
unsafe fn try_render_custom_image(
    image_path: &Path,
    _hdc_screen: HDC,
    hdc_mem: HDC,
    scale: &crate::flyout::DpiScale,
) -> Option<(i32, i32, windows::Win32::Graphics::Gdi::HBITMAP)> {
    if !image_path.exists() {
        return None;
    }

    ensure_gdiplus_initialized();

    let path_str = image_path.to_str()?;
    let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    let mut image: *mut GpImage = std::ptr::null_mut();
    if GdipLoadImageFromFile(PCWSTR(wide_path.as_ptr()), &mut image) != Status(0) || image.is_null() {
        return None;
    }

    let mut w_u32 = 0u32;
    let mut h_u32 = 0u32;
    let _ = GdipGetImageWidth(image, &mut w_u32);
    let _ = GdipGetImageHeight(image, &mut h_u32);

    let raw_w = w_u32 as i32;
    let raw_h = h_u32 as i32;

    if raw_w <= 0 || raw_h <= 0 {
        let _ = GdipDisposeImage(image);
        return None;
    }

    let width = scale.scale(raw_w);
    let height = scale.scale(raw_h);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut p_bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let h_bitmap = match CreateDIBSection(
        hdc_mem,
        &bmi,
        DIB_RGB_COLORS,
        &mut p_bits,
        None,
        0,
    ) {
        Ok(bm) => bm,
        Err(_) => {
            let _ = GdipDisposeImage(image);
            return None;
        }
    };

    let old_bm = SelectObject(hdc_mem, h_bitmap);

    let mut graphics: *mut windows::Win32::Graphics::GdiPlus::GpGraphics = std::ptr::null_mut();
    let _ = GdipCreateFromHDC(hdc_mem, &mut graphics);

    if !graphics.is_null() {
        let _ = GdipDrawImageRectI(graphics, image, 0, 0, width, height);
        let _ = GdipDeleteGraphics(graphics);
    }

    let _ = GdipDisposeImage(image);
    let _ = SelectObject(hdc_mem, old_bm);

    Some((width, height, h_bitmap))
}

/// Initializes the singleton floating HUD window
pub fn init_hud() -> Option<HWND> {
    let existing = HUD_HWND_VAL.load(Ordering::Relaxed);
    if existing != 0 {
        return Some(HWND(existing as _));
    }

    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(hud_wnd_proc),
        lpszClassName: HUD_CLASS_NAME,
        ..Default::default()
    };

    unsafe {
        let _ = RegisterClassW(&wnd_class);

        let ex_style = WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_LAYERED
            | WS_EX_NOACTIVATE
            | WS_EX_TRANSPARENT;

        let hwnd = CreateWindowExW(
            ex_style,
            HUD_CLASS_NAME,
            w!("WintilPinHud"),
            WS_POPUP,
            0,
            0,
            DEFAULT_HUD_WIDTH,
            DEFAULT_HUD_HEIGHT,
            HWND(std::ptr::null_mut()),
            None,
            None,
            None,
        ).ok()?;

        HUD_HWND_VAL.store(hwnd.0 as isize, Ordering::Relaxed);
        Some(hwnd)
    }
}

/// Displays the floating HUD notification with declarative styling.
/// Renders custom image if present in assets/ (pinned.png / unpinned.png);
/// otherwise renders the clean rectangular box with declarative colors and text.
pub fn show_pin_hud(target_hwnd: HWND, pinned: bool, duration_ms: u64, config: &crate::config::PinHudConfig) {
    let hud_hwnd = match init_hud() {
        Some(h) => h,
        None => return,
    };

    let scale = if !target_hwnd.0.is_null() && unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(target_hwnd).as_bool() } {
        crate::flyout::DpiScale::for_window(target_hwnd)
    } else {
        crate::flyout::DpiScale::for_window(hud_hwnd)
    };

    unsafe {
        let hdc_screen = windows::Win32::Graphics::Gdi::GetDC(HWND(std::ptr::null_mut()));
        let hdc_mem = CreateCompatibleDC(hdc_screen);

        // Check for custom image support (e.g. assets/pinned.png or assets/unpinned.png)
        let candidate_image = if pinned {
            Path::new("assets/pinned.png")
        } else {
            Path::new("assets/unpinned.png")
        };

        let (hud_width, hud_height, h_bitmap) = if let Some(res) = try_render_custom_image(candidate_image, hdc_screen, hdc_mem, &scale) {
            res
        } else {
            // Default: Render rectangular box with declarative styling (colors, text, opacity)
            let width = scale.scale(DEFAULT_HUD_WIDTH);
            let height = scale.scale(DEFAULT_HUD_HEIGHT);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // Top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut p_bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let bm = match CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut p_bits,
                None,
                0,
            ) {
                Ok(b) => b,
                Err(_) => {
                    let _ = DeleteDC(hdc_mem);
                    let _ = windows::Win32::Graphics::Gdi::ReleaseDC(HWND(std::ptr::null_mut()), hdc_screen);
                    return;
                }
            };

            // Select DIB bitmap into hdc_mem so GDI operations (DrawTextW) write directly to the buffer
            let prev_bm = SelectObject(hdc_mem, bm);

            let pixel_slice = std::slice::from_raw_parts_mut(
                p_bits as *mut u32,
                (width * height) as usize,
            );

            render_badge(pixel_slice, width, height, pinned, hdc_mem, config, &scale);

            let _ = SelectObject(hdc_mem, prev_bm);
            (width, height, bm)
        };

        let old_bm = SelectObject(hdc_mem, h_bitmap);

        // Placement contextually centered above target window
        let (pos_x, pos_y) = if let Some(target_rect) = crate::window::get_visible_window_bounds(target_hwnd) {
            let center_x = (target_rect.left + target_rect.right) / 2;
            let target_top = target_rect.top;
            let target_h = target_rect.bottom - target_rect.top;
            let y_offset = if target_h < 700 {
                scale.scale(32)
            } else {
                scale.scale(16)
            };
            (center_x - (hud_width / 2), target_top + y_offset)
        } else {
            // Fallback: centered horizontally near the top of primary work area
            let mut work_area = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
                0,
                Some(&mut work_area as *mut _ as *mut _),
                windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            let center_x = (work_area.left + work_area.right) / 2;
            (center_x - (hud_width / 2), work_area.top + scale.scale(80))
        };

        let pt_dst = POINT { x: pos_x, y: pos_y };
        let size_hud = SIZE {
            cx: hud_width,
            cy: hud_height,
        };
        let pt_src = POINT { x: 0, y: 0 };

        let blend = BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1, // AC_SRC_ALPHA (per-pixel alpha)
        };

        let _ = UpdateLayeredWindow(
            hud_hwnd,
            hdc_screen,
            Some(&pt_dst),
            Some(&size_hud),
            hdc_mem,
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(hdc_mem, old_bm);
        let _ = DeleteObject(h_bitmap);
        let _ = DeleteDC(hdc_mem);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(HWND(std::ptr::null_mut()), hdc_screen);

        // Position and show without stealing focus
        let _ = SetWindowPos(
            hud_hwnd,
            HWND_TOPMOST,
            pos_x,
            pos_y,
            hud_width,
            hud_height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        let _ = ShowWindow(hud_hwnd, SW_SHOWNOACTIVATE);

        // Reset auto-dismiss timer
        let _ = KillTimer(hud_hwnd, HUD_TIMER_ID);
        let _ = SetTimer(
            hud_hwnd,
            HUD_TIMER_ID,
            duration_ms.max(400) as u32,
            None,
        );
    }
}
