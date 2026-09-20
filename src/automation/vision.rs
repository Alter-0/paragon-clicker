use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, SRCCOPY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::process::find_process_window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedBoard {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
}

/// Captures the client area of a window using desktop DC composition.
/// Returns (BGRA pixel buffer, width, height, screen_top_left_point).
pub fn capture_window_client_area(hwnd: HWND) -> Result<(Vec<u8>, usize, usize, POINT), String> {
    unsafe {
        let mut client_rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client_rect) == 0 {
            return Err("无法获取窗口客户区尺寸 (GetClientRect 失败)".to_string());
        }

        let width = (client_rect.right - client_rect.left) as i32;
        let height = (client_rect.bottom - client_rect.top) as i32;

        if width < 300 || height < 300 {
            return Err(format!("窗口尺寸过小或已最小化 ({width}x{height})"));
        }

        let mut pt_screen = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut pt_screen) == 0 {
            return Err("无法将窗口坐标转换为屏幕坐标 (ClientToScreen 失败)".to_string());
        }

        let hdc_screen = GetDC(0);
        if hdc_screen == 0 {
            return Err("无法获取屏幕设备上下文 (GetDC 失败)".to_string());
        }

        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem == 0 {
            ReleaseDC(0, hdc_screen);
            return Err("无法创建内存 DC".to_string());
        }

        let hbm = CreateCompatibleBitmap(hdc_screen, width, height);
        if hbm == 0 {
            DeleteDC(hdc_mem);
            ReleaseDC(0, hdc_screen);
            return Err("无法创建位图缓冲区".to_string());
        }

        let old_bmp = SelectObject(hdc_mem, hbm);

        // Copy from Desktop DC at window screen position to guarantee capturing hardware-accelerated/DirectX windows
        BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            hdc_screen,
            pt_screen.x,
            pt_screen.y,
            SRCCOPY,
        );

        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = width;
        bi.bmiHeader.biHeight = -height; // Top-down DIB
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32; // 32-bit BGRA
        bi.bmiHeader.biCompression = BI_RGB;

        let buf_size = (width as usize) * (height as usize) * 4;
        let mut pixels: Vec<u8> = vec![0; buf_size];

        let lines_copied = GetDIBits(
            hdc_mem,
            hbm,
            0,
            height as u32,
            pixels.as_mut_ptr() as *mut _,
            &mut bi,
            DIB_RGB_COLORS,
        );

        // Clean up GDI objects
        SelectObject(hdc_mem, old_bmp);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(0, hdc_screen);

        if lines_copied == 0 {
            return Err("读取位图像素失败 (GetDIBits 返回 0)".to_string());
        }

        Ok((pixels, width as usize, height as usize, pt_screen))
    }
}

/// Detects the 21x21 paragon board square in the given BGRA pixel buffer.
/// Returns client-relative (left, top, right, bottom).
pub fn detect_paragon_board(pixels: &[u8], width: usize, height: usize) -> Option<DetectedBoard> {
    if width < 300 || height < 300 {
        return None;
    }

    // Ignore left glyph/attribute panel (x < 22%) and top menu/tabs (y < 8%) and bottom bar
    let min_x = (width as f64 * 0.20) as usize;
    let max_x = (width as f64 * 0.98) as usize;
    let min_y = (height as f64 * 0.08) as usize;
    let max_y = (height as f64 * 0.92) as usize;

    let span_x = max_x - min_x;
    let span_y = max_y - min_y;

    let mut proj_x = vec![0i64; span_x];
    let mut proj_y = vec![0i64; span_y];

    // Stride 2 for extreme speed (~0.1ms execution) while preserving pixel accuracy
    let step = 2;
    for y in (min_y..max_y).step_by(step) {
        let row_offset = y * width * 4;
        let y_idx = y - min_y;
        for x in (min_x..max_x).step_by(step) {
            let offset = row_offset + x * 4;
            let b = pixels[offset] as i64;
            let g = pixels[offset + 1] as i64;
            let r = pixels[offset + 2] as i64;

            let max_gb = g.max(b);
            let red_diff = r - max_gb;
            // Red glowing boundary line threshold
            if r > 35 && red_diff > 15 {
                proj_x[x - min_x] += red_diff;
                proj_y[y_idx] += red_diff;
            }
        }
    }

    // Find local peaks in proj_x (vertical line candidates)
    let min_peak_score_x = (span_y as f64 * 6.0) as i64;
    let mut peaks_x = Vec::new();
    for i in 2..(span_x - 2) {
        let val = proj_x[i];
        if val > min_peak_score_x
            && val >= proj_x[i - 1]
            && val >= proj_x[i + 1]
            && val >= proj_x[i - 2]
            && val >= proj_x[i + 2]
        {
            peaks_x.push((i + min_x) as i32);
        }
    }

    // Find local peaks in proj_y (horizontal line candidates)
    let min_peak_score_y = (span_x as f64 * 6.0) as i64;
    let mut peaks_y = Vec::new();
    for j in 2..(span_y - 2) {
        let val = proj_y[j];
        if val > min_peak_score_y
            && val >= proj_y[j - 1]
            && val >= proj_y[j + 1]
            && val >= proj_y[j - 2]
            && val >= proj_y[j + 2]
        {
            peaks_y.push((j + min_y) as i32);
        }
    }

    // Board square sizing: between 35% and 95% of screen height
    let min_board_size = (height as f64 * 0.35) as i32;
    let max_board_size = (height as f64 * 0.95) as i32;

    // Viewport target center (around 60% width, 50% height)
    let target_center_x = (width as f64 * 0.60) as i32;
    let target_center_y = (height as f64 * 0.50) as i32;

    let mut candidates = Vec::new();

    for &x1 in &peaks_x {
        for &x2 in &peaks_x {
            let w = x2 - x1;
            if w >= min_board_size && w <= max_board_size {
                for &y1 in &peaks_y {
                    for &y2 in &peaks_y {
                        let h = y2 - y1;
                        let diff = (w - h).abs();
                        // 1:1 square constraint (allow max 4px or 2.5% tolerance)
                        let max_allowed_diff = (w as f64 * 0.025).max(4.0) as i32;
                        if diff <= max_allowed_diff {
                            let cx = (x1 + x2) / 2;
                            let cy = (y1 + y2) / 2;
                            let center_dist = (cx - target_center_x).abs() + (cy - target_center_y).abs();
                            candidates.push((
                                center_dist,
                                DetectedBoard {
                                    left: x1,
                                    top: y1,
                                    right: x2,
                                    bottom: y2,
                                    width: w,
                                    height: h,
                                },
                            ));
                        }
                    }
                }
            }
        }
    }

    // Sort by distance to viewport center (primary board in focus)
    candidates.sort_by_key(|&(dist, _)| dist);
    candidates.into_iter().next().map(|(_, b)| b)
}

/// Automatically captures the target process window, detects the 21x21 board,
/// and returns the absolute screen coordinates (left, top, right, bottom).
pub fn auto_detect_board_from_process(process_name: &str) -> Result<(i32, i32, i32, i32), String> {
    let hwnd = find_process_window(process_name)
        .ok_or_else(|| format!("未找到进程「{process_name}」的可见窗口，请确认游戏已启动。"))?;

    let (pixels, width, height, pt_screen) = capture_window_client_area(hwnd)?;
    let board = detect_paragon_board(&pixels, width, height)
        .ok_or_else(|| "在当前画面中未检测到暗黑4巅峰盘红色边界线，请确认巅峰盘界面已打开并处于视野中。".to_string())?;

    // Convert client-relative coordinates to absolute screen coordinates
    let screen_left = board.left + pt_screen.x;
    let screen_top = board.top + pt_screen.y;
    let screen_right = board.right + pt_screen.x;
    let screen_bottom = board.bottom + pt_screen.y;

    Ok((screen_left, screen_top, screen_right, screen_bottom))
}
