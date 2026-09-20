use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct EnumContext<'a> {
    target_name: &'a str,
    found_hwnd: Option<HWND>,
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam as *mut EnumContext);

    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    if IsIconic(hwnd) != 0 {
        return TRUE;
    }
    if GetWindowTextLengthW(hwnd) <= 0 {
        return TRUE;
    }

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return TRUE;
    }

    let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process_handle == 0 {
        return TRUE;
    }

    let mut path_buf = [0u16; 1024];
    let mut size = path_buf.len() as u32;
    let success = QueryFullProcessImageNameW(process_handle, 0, path_buf.as_mut_ptr(), &mut size);
    CloseHandle(process_handle);

    if success != 0 {
        let exe_path_os = String::from_utf16_lossy(&path_buf[..size as usize]);
        let exe_name = Path::new(&exe_path_os)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default();

        if exe_name.eq_ignore_ascii_case(ctx.target_name) {
            ctx.found_hwnd = Some(hwnd);
            return FALSE; // Stop enumeration
        }
    }

    TRUE
}

pub fn find_process_window(target_process_name: &str) -> Option<HWND> {
    let normalized = target_process_name.trim();
    if normalized.is_empty() {
        return None;
    }

    let mut ctx = EnumContext {
        target_name: normalized,
        found_hwnd: None,
    };

    unsafe {
        EnumWindows(Some(enum_windows_callback), &mut ctx as *mut _ as isize);
        ctx.found_hwnd
    }
}

pub fn activate_process_window(target_process_name: &str) -> bool {
    if let Some(hwnd) = find_process_window(target_process_name) {
        unsafe {
            SetForegroundWindow(hwnd);
            sleep(Duration::from_millis(200));
        }
        true
    } else {
        false
    }
}
