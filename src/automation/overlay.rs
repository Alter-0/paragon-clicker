use std::sync::mpsc::Sender;
use std::thread;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, FillRect,
    InvalidateRect, LineTo, MoveToEx, SelectObject, SetBkMode, SetTextColor, TextOutW, HDC,
    HFONT, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, PostQuitMessage, RegisterClassW, SetCursor,
    SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, IDC_CROSS, LWA_ALPHA, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, WM_DESTROY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
    WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

use crate::d2core::geometry::GRID_SIZE;

unsafe fn create_overlay_font(size: i32) -> HFONT {
    let font_name: Vec<u16> = "Microsoft YaHei\0".encode_utf16().collect();
    CreateFontW(
        size,
        0,
        0,
        0,
        600,
        0,
        0,
        0,
        1, // DEFAULT_CHARSET
        0,
        0,
        4, // CLEARTYPE_QUALITY
        0,
        font_name.as_ptr(),
    )
}

struct SelectionState {
    tx: Option<Sender<Option<(i32, i32, i32, i32)>>>,
    start: Option<POINT>,
    curr: Option<POINT>,
    dragging: bool,
    preview_cells: Vec<(usize, usize)>,
}

unsafe extern "system" fn selection_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam as *const CREATESTRUCTW;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SelectionState;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *ptr;

    match msg {
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(0, IDC_CROSS));
            1
        }
        WM_LBUTTONDOWN => {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            state.start = Some(pt);
            state.curr = Some(pt);
            state.dragging = true;
            SetCapture(hwnd);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_MOUSEMOVE => {
            if state.dragging {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                state.curr = Some(pt);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            if state.dragging {
                ReleaseCapture();
                state.dragging = false;
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);

                if let Some(start) = state.start {
                    let min_x = start.x.min(pt.x);
                    let max_x = start.x.max(pt.x);
                    let min_y = start.y.min(pt.y);
                    let max_y = start.y.max(pt.y);

                    if (max_x - min_x) > 20 && (max_y - min_y) > 20 {
                        if let Some(ref tx) = state.tx {
                            let _ = tx.send(Some((min_x, min_y, max_x, max_y)));
                        }
                    } else if let Some(ref tx) = state.tx {
                        let _ = tx.send(None);
                    }
                }
                DestroyWindow(hwnd);
            }
            0
        }
        WM_RBUTTONDOWN => {
            if let Some(ref tx) = state.tx {
                let _ = tx.send(None);
            }
            DestroyWindow(hwnd);
            0
        }
        WM_KEYDOWN => {
            if wparam == VK_ESCAPE as usize {
                if let Some(ref tx) = state.tx {
                    let _ = tx.send(None);
                }
                DestroyWindow(hwnd);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let bg_brush = CreateSolidBrush(0x00100805);
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: vw,
                bottom: vh,
            };
            FillRect(hdc, &full_rect, bg_brush);
            DeleteObject(bg_brush);

            let title_font = create_overlay_font(20);
            let old_font = SelectObject(hdc, title_font);

            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00FFFFFF);
            let text: Vec<u16> = "【选区模式】请从游戏巅峰盘左上角按住鼠标左键，拖拽到右下角松开。按 Esc 或右键取消。"
                .encode_utf16()
                .collect();
            TextOutW(hdc, 30, 30, text.as_ptr(), text.len() as i32);

            if let (Some(start), Some(curr)) = (state.start, state.curr) {
                let l = start.x.min(curr.x) - vx;
                let r = start.x.max(curr.x) - vx;
                let t = start.y.min(curr.y) - vy;
                let b = start.y.max(curr.y) - vy;

                let sel_brush = CreateSolidBrush(0x00603015);
                let sel_rect = RECT { left: l, top: t, right: r, bottom: b };
                FillRect(hdc, &sel_rect, sel_brush);
                DeleteObject(sel_brush);

                let border_pen = CreatePen(PS_SOLID, 2, 0x00FFA27A);
                let old_pen = SelectObject(hdc, border_pen);
                MoveToEx(hdc, l, t, std::ptr::null_mut());
                LineTo(hdc, r, t);
                LineTo(hdc, r, b);
                LineTo(hdc, l, b);
                LineTo(hdc, l, t);
                SelectObject(hdc, old_pen);
                DeleteObject(border_pen);

                let w = r - l;
                let h = b - t;
                if w > 20 && h > 20 {
                    let cell_w = w as f64 / GRID_SIZE as f64;
                    let cell_h = h as f64 / GRID_SIZE as f64;

                    let grid_pen = CreatePen(PS_SOLID, 1, 0x00FFE861);
                    let old_grid_pen = SelectObject(hdc, grid_pen);
                    for i in 1..GRID_SIZE {
                        let gx = l + (i as f64 * cell_w).round() as i32;
                        MoveToEx(hdc, gx, t, std::ptr::null_mut());
                        LineTo(hdc, gx, b);

                        let gy = t + (i as f64 * cell_h).round() as i32;
                        MoveToEx(hdc, l, gy, std::ptr::null_mut());
                        LineTo(hdc, r, gy);
                    }
                    SelectObject(hdc, old_grid_pen);
                    DeleteObject(grid_pen);

                    let dot_brush = CreateSolidBrush(0x005CD0FF);
                    let old_brush = SelectObject(hdc, dot_brush);
                    let dot_pen = CreatePen(PS_SOLID, 1, 0x005CD0FF);
                    let old_dot_pen = SelectObject(hdc, dot_pen);

                    let label_font = create_overlay_font(15);
                    let old_lbl_font = SelectObject(hdc, label_font);

                    for (idx, &(row, col)) in state.preview_cells.iter().enumerate() {
                        let cx = l + ((col as f64 + 0.5) * cell_w).round() as i32;
                        let cy = t + ((row as f64 + 0.5) * cell_h).round() as i32;
                        Ellipse(hdc, cx - 4, cy - 4, cx + 5, cy + 5);

                        if idx == 0 {
                            SetTextColor(hdc, 0x00A2FF87);
                            let start_txt: Vec<u16> = "起点".encode_utf16().collect();
                            TextOutW(hdc, cx + 6, cy - 14, start_txt.as_ptr(), start_txt.len() as i32);
                        } else if idx == state.preview_cells.len() - 1 {
                            SetTextColor(hdc, 0x008F8FFF);
                            let end_txt: Vec<u16> = "终点".encode_utf16().collect();
                            TextOutW(hdc, cx + 6, cy - 14, end_txt.as_ptr(), end_txt.len() as i32);
                        }
                    }

                    SelectObject(hdc, old_lbl_font);
                    DeleteObject(label_font);

                    SelectObject(hdc, old_brush);
                    SelectObject(hdc, old_dot_pen);
                    DeleteObject(dot_brush);
                    DeleteObject(dot_pen);
                }
            }

            SelectObject(hdc, old_font);
            DeleteObject(title_font);

            EndPaint(hwnd, &ps);
            0
        }
        WM_NCDESTROY => {
            let _ = Box::from_raw(ptr);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn open_native_selection_overlay(
    preview_cells: Vec<(usize, usize)>,
    tx: Sender<Option<(i32, i32, i32, i32)>>,
) {
    thread::spawn(move || unsafe {
        let state = Box::new(SelectionState {
            tx: Some(tx),
            start: None,
            curr: None,
            dragging: false,
            preview_cells,
        });

        let class_name: Vec<u16> = "ParagonSelectionOverlay\0".encode_utf16().collect();
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(selection_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: 0,
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_CROSS),
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            0,
            0,
            0,
            Box::into_raw(state) as *mut _,
        );

        if hwnd == 0 {
            return;
        }

        SetLayeredWindowAttributes(hwnd, 0, 115, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOW);

        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

// -------------------------------------------------------------
// Grid Preview Overlay
// -------------------------------------------------------------

struct PreviewState {
    region: (i32, i32, i32, i32),
    points: Vec<(i32, i32)>,
}

unsafe extern "system" fn preview_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam as *const CREATESTRUCTW;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PreviewState;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &*ptr;

    match msg {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            DestroyWindow(hwnd);
            0
        }
        WM_KEYDOWN => {
            if wparam == VK_ESCAPE as usize {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let bg_brush = CreateSolidBrush(0x00100805);
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: vw,
                bottom: vh,
            };
            FillRect(hdc, &full_rect, bg_brush);
            DeleteObject(bg_brush);

            let title_font = create_overlay_font(20);
            let old_font = SelectObject(hdc, title_font);

            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00FFFFFF);
            let text: Vec<u16> = "【21x21 网格预览】青色线为格子边界，金色点为点击位置。单击任意处或按 Esc 退出。"
                .encode_utf16()
                .collect();
            TextOutW(hdc, 30, 30, text.as_ptr(), text.len() as i32);

            let (phys_l, phys_t, phys_r, phys_b) = state.region;
            let l = phys_l - vx;
            let r = phys_r - vx;
            let t = phys_t - vy;
            let b = phys_b - vy;

            let sel_brush = CreateSolidBrush(0x00402010);
            let sel_rect = RECT { left: l, top: t, right: r, bottom: b };
            FillRect(hdc, &sel_rect, sel_brush);
            DeleteObject(sel_brush);

            let border_pen = CreatePen(PS_SOLID, 2, 0x00FFA27A);
            let old_pen = SelectObject(hdc, border_pen);
            MoveToEx(hdc, l, t, std::ptr::null_mut());
            LineTo(hdc, r, t);
            LineTo(hdc, r, b);
            LineTo(hdc, l, b);
            LineTo(hdc, l, t);
            SelectObject(hdc, old_pen);
            DeleteObject(border_pen);

            let w = r - l;
            let h = b - t;
            if w > 20 && h > 20 {
                let cell_w = w as f64 / GRID_SIZE as f64;
                let cell_h = h as f64 / GRID_SIZE as f64;

                let grid_pen = CreatePen(PS_SOLID, 1, 0x00FFE861);
                let old_grid_pen = SelectObject(hdc, grid_pen);
                for i in 1..GRID_SIZE {
                    let gx = l + (i as f64 * cell_w).round() as i32;
                    MoveToEx(hdc, gx, t, std::ptr::null_mut());
                    LineTo(hdc, gx, b);

                    let gy = t + (i as f64 * cell_h).round() as i32;
                    MoveToEx(hdc, l, gy, std::ptr::null_mut());
                    LineTo(hdc, r, gy);
                }
                SelectObject(hdc, old_grid_pen);
                DeleteObject(grid_pen);

                let dot_brush = CreateSolidBrush(0x005CD0FF);
                let old_brush = SelectObject(hdc, dot_brush);
                let dot_pen = CreatePen(PS_SOLID, 1, 0x005CD0FF);
                let old_dot_pen = SelectObject(hdc, dot_pen);

                let label_font = create_overlay_font(15);
                let old_lbl_font = SelectObject(hdc, label_font);

                for (idx, &(px, py)) in state.points.iter().enumerate() {
                    let cx = px - vx;
                    let cy = py - vy;
                    Ellipse(hdc, cx - 4, cy - 4, cx + 5, cy + 5);

                    if idx == 0 {
                        SetTextColor(hdc, 0x00A2FF87);
                        let start_txt: Vec<u16> = "起点".encode_utf16().collect();
                        TextOutW(hdc, cx + 6, cy - 14, start_txt.as_ptr(), start_txt.len() as i32);
                    } else if idx == state.points.len() - 1 {
                        SetTextColor(hdc, 0x008F8FFF);
                        let end_txt: Vec<u16> = "终点".encode_utf16().collect();
                        TextOutW(hdc, cx + 6, cy - 14, end_txt.as_ptr(), end_txt.len() as i32);
                    }
                }

                SelectObject(hdc, old_lbl_font);
                DeleteObject(label_font);

                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_dot_pen);
                DeleteObject(dot_brush);
                DeleteObject(dot_pen);
            }

            SelectObject(hdc, old_font);
            DeleteObject(title_font);

            EndPaint(hwnd, &ps);
            0
        }
        WM_NCDESTROY => {
            let _ = Box::from_raw(ptr);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn open_native_grid_preview(
    region: (i32, i32, i32, i32),
    points: Vec<(i32, i32)>,
) {
    thread::spawn(move || unsafe {
        let state = Box::new(PreviewState { region, points });

        let class_name: Vec<u16> = "ParagonGridPreview\0".encode_utf16().collect();
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(preview_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: 0,
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_CROSS),
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            0,
            0,
            0,
            Box::into_raw(state) as *mut _,
        );

        if hwnd == 0 {
            return;
        }

        SetLayeredWindowAttributes(hwnd, 0, 115, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOW);

        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
