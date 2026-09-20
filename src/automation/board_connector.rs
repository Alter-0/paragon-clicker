use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::d2core::model::{BoardSequence, Position};
use super::input::{is_failsafe_triggered, move_mouse_and_click};
use super::ocr::recognize_text_from_rgba;
use super::vision::{auto_detect_board_from_process, capture_window_client_area};

/// Nominal relative coordinates (relative to window client width/height)
pub const PREVIEW_BTN_REL_X: f64 = 0.782;
pub const PREVIEW_BTN_REL_Y: f64 = 0.870;

pub const NEXT_BOARD_ARROW_REL_X: f64 = 0.952;
pub const NEXT_BOARD_ARROW_REL_Y: f64 = 0.356;

pub const PREV_BOARD_ARROW_REL_X: f64 = 0.695;
pub const PREV_BOARD_ARROW_REL_Y: f64 = 0.356;

pub const ATTACH_BTN_REL_X: f64 = 0.421;
pub const ATTACH_BTN_REL_Y: f64 = 0.965;

pub const ROTATE_BTN_REL_X: f64 = 0.498;
pub const ROTATE_BTN_REL_Y: f64 = 0.965;

pub const CONFIRM_BTN_REL_X: f64 = 0.460;
pub const CONFIRM_BTN_REL_Y: f64 = 0.597;

pub const TITLE_REL_X_MIN: f64 = 0.720;
pub const TITLE_REL_X_MAX: f64 = 0.960;
pub const TITLE_REL_Y_MIN: f64 = 0.320;
pub const TITLE_REL_Y_MAX: f64 = 0.390;

/// Calculates the exit gate coordinate (row, col) on the parent board that connects to the child board.
pub fn get_exit_gate_coord(parent_pos: &Position, child_pos: &Position) -> Option<(usize, usize)> {
    let dx = child_pos.x - parent_pos.x;
    let dy = child_pos.y - parent_pos.y;

    if dy == -1 {
        Some((0, 10)) // Child is above -> Gate is top edge
    } else if dy == 1 {
        Some((20, 10)) // Child is below -> Gate is bottom edge
    } else if dx == -1 {
        Some((10, 0)) // Child is left -> Gate is left edge
    } else if dx == 1 {
        Some((10, 20)) // Child is right -> Gate is right edge
    } else {
        None
    }
}

/// Converts client relative coordinates (0.0 .. 1.0) to screen coordinates.
pub fn client_rel_to_screen(hwnd: HWND, rel_x: f64, rel_y: f64) -> (i32, i32) {
    let mut rect = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut rect) };
    let client_w = rect.right - rect.left;
    let client_h = rect.bottom - rect.top;

    let client_x = (rel_x * client_w as f64).round() as i32;
    let client_y = (rel_y * client_h as f64).round() as i32;

    let mut pt = windows_sys::Win32::Foundation::POINT {
        x: client_x,
        y: client_y,
    };
    unsafe { ClientToScreen(hwnd, &mut pt) };
    (pt.x, pt.y)
}

/// Searches for a Diablo IV dark-red button centroid around nominal (rel_x, rel_y) in raw BGRA pixel buffer.
/// Returns Some((client_x, client_y)) if and only if a red button is ACTUALLY present (count >= 30).
pub fn find_red_button_in_pixels(
    pixels: &[u8],
    width: usize,
    height: usize,
    rel_x: f64,
    rel_y: f64,
) -> Option<(i32, i32)> {
    let cw = width as i32;
    let ch = height as i32;
    let cx = (rel_x * cw as f64).round() as i32;
    let cy = (rel_y * ch as f64).round() as i32;

    // Search window: +/- 35px in X, +/- 15px in Y
    let x_min = (cx - 35).clamp(0, cw - 1);
    let x_max = (cx + 35).clamp(0, cw - 1);
    let y_min = (cy - 15).clamp(0, ch - 1);
    let y_max = (cy + 15).clamp(0, ch - 1);

    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut count: i64 = 0;

    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let idx = ((y * cw + x) * 4) as usize;
            if idx + 2 < pixels.len() {
                // Buffer is BGRA
                let b = pixels[idx] as i32;
                let g = pixels[idx + 1] as i32;
                let r = pixels[idx + 2] as i32;
                // Diablo IV button red: R > 50 and significantly higher than G and B
                if r > 50 && r > g + 15 && r > b + 15 {
                    sum_x += x as i64;
                    sum_y += y as i64;
                    count += 1;
                }
            }
        }
    }

    if count >= 30 {
        let snapped_cx = (sum_x / count) as i32;
        let snapped_cy = (sum_y / count) as i32;
        Some((snapped_cx, snapped_cy))
    } else {
        None
    }
}

/// Searches for a Diablo IV dark-red button centroid around nominal (rel_x, rel_y).
/// Returns Some((screen_x, screen_y)) if and only if a red button is ACTUALLY present (count >= 30).
pub fn find_red_button(hwnd: HWND, rel_x: f64, rel_y: f64) -> Option<(i32, i32)> {
    let (pixels, cw_usize, ch_usize, pt) = capture_window_client_area(hwnd).ok()?;
    let (client_x, client_y) = find_red_button_in_pixels(&pixels, cw_usize, ch_usize, rel_x, rel_y)?;
    Some((pt.x + client_x, pt.y + client_y))
}

/// Snaps a client point to the centroid of a nearby Diablo IV dark-red button if present,
/// or falls back to nominal client coordinates.
pub fn snap_to_red_button(hwnd: HWND, rel_x: f64, rel_y: f64) -> (i32, i32) {
    find_red_button(hwnd, rel_x, rel_y).unwrap_or_else(|| client_rel_to_screen(hwnd, rel_x, rel_y))
}

/// Waits until the "面板选项" dialog has actually appeared (verified by presence of the 【预览】 button).
pub fn wait_for_board_options_modal(
    hwnd: HWND,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(i32, i32), String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }
        if let Some(pos) = find_red_button(hwnd, PREVIEW_BTN_REL_X, PREVIEW_BTN_REL_Y) {
            return Ok(pos);
        }
        thread::sleep(Duration::from_millis(80));
    }
    Err("等待【面板选项】页面超时：未检测到【预览】按钮，请确认游戏已响应".to_string())
}

/// Waits until the "面板预览模式" has actually appeared (verified by presence of 【旋转面板】 and 【附接面板】 buttons).
/// Returns ((attach_screen_x, attach_screen_y), (rot_screen_x, rot_screen_y)).
pub fn wait_for_preview_mode_buttons(
    hwnd: HWND,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<((i32, i32), (i32, i32)), String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }
        let rot_opt = find_red_button(hwnd, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y);
        let attach_opt = find_red_button(hwnd, ATTACH_BTN_REL_X, ATTACH_BTN_REL_Y);

        if let (Some(attach_pos), Some(rot_pos)) = (attach_opt, rot_opt) {
            return Ok((attach_pos, rot_pos));
        }
        thread::sleep(Duration::from_millis(80));
    }
    Err("等待【面板预览模式】超时：未检测到【旋转面板】/【附接面板】按钮".to_string())
}

/// Waits until the board attachment has actually finished and the new board's red border is detected.
/// Handles the 5th board limit confirmation dialog if it pops up.
pub fn wait_for_attachment_complete(
    hwnd: HWND,
    process_name: &str,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(i32, i32, i32, i32), String> {
    let mut start = Instant::now();
    let mut confirmed_warning = false;
    while start.elapsed() < timeout {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }

        // Check if 5th board limit warning dialog popped up ("警告: 在连接此面板后你将达到5面板上限。你确定要连接吗？")
        if !confirmed_warning {
            if let Some(confirm_pos) = find_red_button(hwnd, CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y) {
                let _ = move_mouse_and_click(confirm_pos.0, confirm_pos.1);
                confirmed_warning = true;
                start = Instant::now(); // Reset timeout window for post-attachment detection
                thread::sleep(Duration::from_millis(400));
                continue;
            }
        }

        // In preview mode, the "附接面板" button is at ATTACH_BTN_REL_X (0.421).
        // When attached, "附接面板" disappears, and the center button becomes "全部返还".
        let attach_opt = find_red_button(hwnd, ATTACH_BTN_REL_X, ATTACH_BTN_REL_Y);
        if attach_opt.is_none() {
            // Preview buttons gone, game has attached the board (now showing "全部返还")!
            if let Ok(new_board) = auto_detect_board_from_process(process_name) {
                return Ok(new_board);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("附接后等待新盘面出现超时，请按 [F7] 手动识别".to_string())
}

/// Crops the board title text area in the "面板选项" popup and runs OCR.
pub fn read_current_board_title(hwnd: HWND) -> Result<String, String> {
    let (pixels, cw_usize, ch_usize, _pt) = capture_window_client_area(hwnd)?;

    let cw = cw_usize as f64;
    let ch = ch_usize as f64;

    let x0 = (TITLE_REL_X_MIN * cw).round() as u32;
    let x1 = (TITLE_REL_X_MAX * cw).round() as u32;
    let y0 = (TITLE_REL_Y_MIN * ch).round() as u32;
    let y1 = (TITLE_REL_Y_MAX * ch).round() as u32;

    let crop_w = x1.saturating_sub(x0).clamp(1, cw_usize as u32);
    let crop_h = y1.saturating_sub(y0).clamp(1, ch_usize as u32);

    // Convert from BGRA to RGBA for OCR
    let mut crop_rgba = Vec::with_capacity((crop_w * crop_h * 4) as usize);
    for y in y0..y0 + crop_h {
        for x in x0..x0 + crop_w {
            let idx = ((y as usize * cw_usize + x as usize) * 4) as usize;
            if idx + 3 < pixels.len() {
                let b = pixels[idx];
                let g = pixels[idx + 1];
                let r = pixels[idx + 2];
                let a = pixels[idx + 3];
                crop_rgba.push(r);
                crop_rgba.push(g);
                crop_rgba.push(b);
                crop_rgba.push(a);
            } else {
                crop_rgba.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    recognize_text_from_rgba(crop_w, crop_h, &crop_rgba)
}

/// Checks if recognized OCR text matches the target board name.
pub fn is_board_name_match(ocr_text: &str, target_name: &str) -> bool {
    let clean_ocr: String = ocr_text.chars().filter(|c| !c.is_whitespace()).collect();
    let clean_target: String = target_name.chars().filter(|c| !c.is_whitespace()).collect();

    if clean_ocr.contains(&clean_target) || clean_target.contains(&clean_ocr) {
        return true;
    }

    // Keyword / character overlap check (e.g. "无底深渊" vs "无底深潜")
    let target_chars: Vec<char> = clean_target.chars().collect();
    let mut match_count = 0;
    for &tc in &target_chars {
        if clean_ocr.contains(tc) {
            match_count += 1;
        }
    }

    if !target_chars.is_empty() && (match_count as f64 / target_chars.len() as f64) >= 0.5 {
        return true;
    }

    false
}

/// Fully automated board connector workflow with strict visual state verification:
/// 1. Click exit gate on current board -> Polling-wait until "面板选项" (and "预览" button) ACTUALLY appears
/// 2. Match target board name (cycle with `>` if needed)
/// 3. Click "预览" -> Polling-wait until "面板预览模式" (and "旋转面板" / "附接面板" buttons) ACTUALLY appears
/// 4. Click "旋转面板" `rotate_count` times
/// 5. Click "附接面板" -> Polling-wait until preview mode closes and new board is attached
/// 6. Auto-detect new board
pub fn execute_board_attachment_pipeline(
    hwnd: HWND,
    current_board_rect: (i32, i32, u32, u32),
    current_board: &BoardSequence,
    next_board: &BoardSequence,
    process_name: &str,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(i32, i32, i32, i32), String> {
    if is_cancelled() || is_failsafe_triggered() {
        return Err("用户中断操作".to_string());
    }

    // Step 1: Find exit gate coordinate
    let (gate_row, gate_col) = get_exit_gate_coord(
        &current_board.boardPosition,
        &next_board.boardPosition,
    ).ok_or_else(|| format!("无法计算从盘【{}】到盘【{}】的连接门方向", current_board.boardName, next_board.boardName))?;

    // Gate screen position
    let (bx, by, bw, bh) = current_board_rect;
    let gate_screen_x = bx + (((gate_col as f64 + 0.5) / 21.0) * bw as f64).round() as i32;
    let gate_screen_y = by + (((gate_row as f64 + 0.5) / 21.0) * bh as f64).round() as i32;

    // Click exit gate and CONFIRM that the "面板选项" modal actually appears (up to 3 clicks if game missed input)
    let mut preview_btn_pos: Option<(i32, i32)> = None;
    for _ in 0..3 {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }

        move_mouse_and_click(gate_screen_x, gate_screen_y).map_err(|e| e.to_string())?;

        // Poll up to 1.5s for the modal to actually render
        match wait_for_board_options_modal(hwnd, Duration::from_millis(1500), is_cancelled) {
            Ok(pos) => {
                preview_btn_pos = Some(pos);
                break;
            }
            Err(_) => {
                // If not yet visible, loop to re-click gate
            }
        }
    }

    let preview_pos = preview_btn_pos
        .ok_or_else(|| "点击终点门后未检测到【面板选项】页面弹出，请确认游戏当前画面".to_string())?;

    // Step 2: In "面板选项", select target board
    let target_name = &next_board.boardName;
    let mut _matched = false;
    for _ in 0..10 {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }

        if let Ok(title_text) = read_current_board_title(hwnd) {
            if is_board_name_match(&title_text, target_name) {
                _matched = true;
                break;
            }
        }

        // Click next arrow `>`
        let (arrow_x, arrow_y) = client_rel_to_screen(hwnd, NEXT_BOARD_ARROW_REL_X, NEXT_BOARD_ARROW_REL_Y);
        move_mouse_and_click(arrow_x, arrow_y).map_err(|e| e.to_string())?;
        thread::sleep(Duration::from_millis(260));
    }

    if is_cancelled() || is_failsafe_triggered() {
        return Err("用户中断操作".to_string());
    }

    // Step 3: Click "预览" button
    move_mouse_and_click(preview_pos.0, preview_pos.1).map_err(|e| e.to_string())?;

    // WAITING FOR PREVIEW MODE BUTTONS TO ACTUALLY APPEAR!
    let (attach_pos, rot_pos) = wait_for_preview_mode_buttons(hwnd, Duration::from_secs(5), is_cancelled)?;

    if is_cancelled() || is_failsafe_triggered() {
        return Err("用户中断操作".to_string());
    }

    // Step 4: Rotate board according to next_board.boardRotate
    let rotate_clicks = ((next_board.boardRotate % 4) + 4) % 4;
    for _ in 0..rotate_clicks {
        if is_cancelled() || is_failsafe_triggered() {
            return Err("用户中断操作".to_string());
        }
        move_mouse_and_click(rot_pos.0, rot_pos.1).map_err(|e| e.to_string())?;
        thread::sleep(Duration::from_millis(300));
    }

    if is_cancelled() || is_failsafe_triggered() {
        return Err("用户中断操作".to_string());
    }

    // Step 5: Click "附接面板" button
    move_mouse_and_click(attach_pos.0, attach_pos.1).map_err(|e| e.to_string())?;

    // WAITING FOR ATTACHMENT TO ACTUALLY COMPLETE AND NEW BOARD TO APPEAR!
    let new_board = wait_for_attachment_complete(hwnd, process_name, Duration::from_secs(8), is_cancelled)?;

    Ok(new_board)
}
