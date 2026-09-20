use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn configure_dpi_awareness() {
    unsafe {
        // Try SetProcessDpiAwarenessContext (Per-Monitor V2 = -4)
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 {
            return;
        }
        // Fallback to SetProcessDPIAware
        SetProcessDPIAware();
    }
}
