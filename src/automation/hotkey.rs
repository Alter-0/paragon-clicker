use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, VK_F7, VK_F8, VK_F9,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY, WM_QUIT,
};

pub const HOTKEY_START_ID: i32 = 0x5001;
pub const HOTKEY_STOP_ID: i32 = 0x5002;
pub const HOTKEY_DETECT_ID: i32 = 0x5003;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Start,
    Stop,
    Detect,
}

pub struct HotkeyListener {
    stop_flag: Arc<AtomicBool>,
}

impl HotkeyListener {
    pub fn new(tx: Sender<HotkeyEvent>) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&stop_flag);

        thread::spawn(move || unsafe {
            // Register F7 (Detect), F8 (Start) and F9 (Stop) without modifier keys
            let reg_detect = RegisterHotKey(0, HOTKEY_DETECT_ID, 0, VK_F7 as u32);
            let reg_start = RegisterHotKey(0, HOTKEY_START_ID, 0, VK_F8 as u32);
            let reg_stop = RegisterHotKey(0, HOTKEY_STOP_ID, 0, VK_F9 as u32);

            let mut msg: MSG = std::mem::zeroed();
            while !flag_clone.load(Ordering::Relaxed) {
                if PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    if msg.message == WM_HOTKEY {
                        match msg.wParam as i32 {
                            HOTKEY_DETECT_ID => {
                                let _ = tx.send(HotkeyEvent::Detect);
                            }
                            HOTKEY_START_ID => {
                                let _ = tx.send(HotkeyEvent::Start);
                            }
                            HOTKEY_STOP_ID => {
                                let _ = tx.send(HotkeyEvent::Stop);
                            }
                            _ => {}
                        }
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    thread::sleep(Duration::from_millis(20));
                }
            }

            if reg_detect != 0 {
                UnregisterHotKey(0, HOTKEY_DETECT_ID);
            }
            if reg_start != 0 {
                UnregisterHotKey(0, HOTKEY_START_ID);
            }
            if reg_stop != 0 {
                UnregisterHotKey(0, HOTKEY_STOP_ID);
            }
        });

        Self { stop_flag }
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
