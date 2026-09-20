use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

use crate::d2core::geometry::GRID_SIZE;
use crate::d2core::model::{BoardSequence, ClickPoint};

pub struct PrecisionTimerGuard {
    active: bool,
}

impl PrecisionTimerGuard {
    pub fn new() -> Self {
        unsafe {
            timeBeginPeriod(1);
        }
        Self { active: true }
    }
}

impl Default for PrecisionTimerGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PrecisionTimerGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                timeEndPeriod(1);
            }
            self.active = false;
        }
    }
}

pub fn is_failsafe_triggered() -> bool {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) != 0 {
            point.x <= 0 && point.y <= 0
        } else {
            false
        }
    }
}

pub fn move_mouse_and_click(x: i32, y: i32) -> Result<(), &'static str> {
    unsafe {
        SetCursorPos(x, y);
        if is_failsafe_triggered() {
            return Err("已触发安全停止");
        }

        let down = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTDOWN,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let up = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        SendInput(1, &down, std::mem::size_of::<INPUT>() as i32);
        thread::sleep(Duration::from_millis(15));
        SendInput(1, &up, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(())
}

pub fn build_click_points(
    board: &BoardSequence,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> Vec<ClickPoint> {
    let width = right - left;
    let height = bottom - top;
    if width <= 0 || height <= 0 {
        return Vec::new();
    }

    let cell_w = width as f64 / GRID_SIZE as f64;
    let cell_h = height as f64 / GRID_SIZE as f64;
    let mut points = Vec::new();

    for step in &board.steps {
        let col = step.rotatedCoord.col;
        let row = step.rotatedCoord.row;
        let px = left as f64 + (col as f64 + 0.5) * cell_w;
        let py = top as f64 + (row as f64 + 0.5) * cell_h;

        points.push(ClickPoint {
            step: step.step,
            local_step: step.localStep.unwrap_or(0),
            node_name: step.nodeName.clone(),
            node_kind: step.nodeKind.clone(),
            board_key: step.boardKey.clone(),
            row,
            col,
            x: px.round() as i32,
            y: py.round() as i32,
        });
    }

    points
}
