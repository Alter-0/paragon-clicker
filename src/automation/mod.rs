pub mod board_connector;
pub mod dpi;
pub mod hotkey;
pub mod input;
pub mod ocr;
pub mod overlay;
pub mod process;
pub mod vision;

pub use board_connector::{
    client_rel_to_screen, execute_board_attachment_pipeline, find_red_button,
    find_red_button_in_pixels, get_exit_gate_coord, is_board_name_match, snap_to_red_button,
    wait_for_attachment_complete, wait_for_board_options_modal, wait_for_preview_mode_buttons,
    CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y,
};
pub use dpi::configure_dpi_awareness;
pub use hotkey::{HotkeyEvent, HotkeyListener};
pub use input::{build_click_points, is_failsafe_triggered, move_mouse_and_click, PrecisionTimerGuard};
pub use ocr::recognize_text_from_rgba;
pub use overlay::{open_native_grid_preview, open_native_selection_overlay};
pub use process::{activate_process_window, find_process_window};
pub use vision::{auto_detect_board_from_process, detect_paragon_board, DetectedBoard};
