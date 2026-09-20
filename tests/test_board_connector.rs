use paragon_clicker_rust::automation::board_connector::{get_exit_gate_coord, is_board_name_match};
use paragon_clicker_rust::d2core::model::Position;

#[test]
fn test_get_exit_gate_all_four_directions() {
    let parent = Position { x: 0, y: 0 };

    // Child is above: dy = -1
    let child_up = Position { x: 0, y: -1 };
    assert_eq!(get_exit_gate_coord(&parent, &child_up), Some((0, 10)));

    // Child is below: dy = 1
    let child_down = Position { x: 0, y: 1 };
    assert_eq!(get_exit_gate_coord(&parent, &child_down), Some((20, 10)));

    // Child is left: dx = -1
    let child_left = Position { x: -1, y: 0 };
    assert_eq!(get_exit_gate_coord(&parent, &child_left), Some((10, 0)));

    // Child is right: dx = 1
    let child_right = Position { x: 1, y: 0 };
    assert_eq!(get_exit_gate_coord(&parent, &child_right), Some((10, 20)));

    // Invalid connection
    let child_far = Position { x: 2, y: 2 };
    assert_eq!(get_exit_gate_coord(&parent, &child_far), None);
}

#[test]
fn test_board_name_matching_fuzzy_and_exact() {
    // Exact substring
    assert!(is_board_name_match("无底深渊 1/9", "无底深渊"));
    assert!(is_board_name_match("无底深渊", "无底深渊 1/9"));

    // Name variant in Diablo IV translation (深渊 vs 深潜)
    assert!(is_board_name_match("无底深渊 1/9", "无底深潜"));

    // Spacing tolerance
    assert!(is_board_name_match("无 底 深 渊 1 / 9", "无底深渊"));

    // Negative matches
    assert!(!is_board_name_match("恶魔尖刺 2/9", "无底深渊"));
    assert!(!is_board_name_match("统治 5/9", "动力"));
}

#[test]
fn test_rotation_counts() {
    for (rotate_input, expected_clicks) in [(0, 0), (1, 1), (2, 2), (3, 3), (4, 0), (-1, 3)] {
        let clicks = ((rotate_input % 4) + 4) % 4;
        assert_eq!(clicks, expected_clicks);
    }
}

#[test]
fn test_multi_board_hierarchy_gates() {
    let b0_pos = Position { x: 0, y: 0 };
    let b1_pos = Position { x: 0, y: -1 }; // Above B0
    let b2_pos = Position { x: 1, y: -1 }; // Right of B1
    let b3_pos = Position { x: 1, y: 0 };  // Below B2

    // B0 -> B1
    assert_eq!(get_exit_gate_coord(&b0_pos, &b1_pos), Some((0, 10)));
    // B1 -> B2
    assert_eq!(get_exit_gate_coord(&b1_pos, &b2_pos), Some((10, 20)));
    // B2 -> B3
    assert_eq!(get_exit_gate_coord(&b2_pos, &b3_pos), Some((20, 10)));
}

#[test]
fn test_find_red_button_in_pixels() {
    use paragon_clicker_rust::automation::board_connector::{
        find_red_button_in_pixels, PREVIEW_BTN_REL_X, PREVIEW_BTN_REL_Y, ROTATE_BTN_REL_X,
        ROTATE_BTN_REL_Y,
    };

    let w = 1000usize;
    let h = 600usize;
    let mut pixels = vec![0u8; w * h * 4]; // all black

    // Initially: no buttons
    assert_eq!(find_red_button_in_pixels(&pixels, w, h, PREVIEW_BTN_REL_X, PREVIEW_BTN_REL_Y), None);
    assert_eq!(find_red_button_in_pixels(&pixels, w, h, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y), None);

    // Paint a "preview" red button at (PREVIEW_BTN_REL_X * 1000 = 782, PREVIEW_BTN_REL_Y * 600 = 522)
    let px = 782usize;
    let py = 522usize;
    for y in py - 5..=py + 5 {
        for x in px - 10..=px + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 10;     // B
            pixels[idx + 1] = 10; // G
            pixels[idx + 2] = 120; // R (bright red)
            pixels[idx + 3] = 255; // A
        }
    }

    // Now preview button is detected!
    let found_prev = find_red_button_in_pixels(&pixels, w, h, PREVIEW_BTN_REL_X, PREVIEW_BTN_REL_Y);
    assert!(found_prev.is_some());
    let (fx, fy) = found_prev.unwrap();
    assert_eq!(fx, px as i32);
    assert_eq!(fy, py as i32);

    // Rotate button is still None
    assert_eq!(find_red_button_in_pixels(&pixels, w, h, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y), None);

    // Paint rotate and attach buttons at bottom
    let rx = (ROTATE_BTN_REL_X * w as f64).round() as usize; // ~498
    let ry = (ROTATE_BTN_REL_Y * h as f64).round() as usize; // ~579
    for y in ry - 5..=ry + 5 {
        for x in rx - 10..=rx + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 10;
            pixels[idx + 1] = 10;
            pixels[idx + 2] = 120;
            pixels[idx + 3] = 255;
        }
    }

    let found_rot = find_red_button_in_pixels(&pixels, w, h, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y);
    assert!(found_rot.is_some());
    assert_eq!(found_rot.unwrap().0, rx as i32);
    assert_eq!(found_rot.unwrap().1, ry as i32);
}

#[test]
fn test_preview_vs_attached_state_distinction() {
    use paragon_clicker_rust::automation::board_connector::{
        find_red_button_in_pixels, ATTACH_BTN_REL_X, ATTACH_BTN_REL_Y, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y,
    };

    let w = 1024usize;
    let h = 576usize;
    let mut pixels = vec![0u8; w * h * 4];

    // Scenario 1: Preview Mode
    // Both attach button (X ~ 431) and rotate button (X ~ 510) are present
    let ax = (ATTACH_BTN_REL_X * w as f64).round() as usize;
    let rx = (ROTATE_BTN_REL_X * w as f64).round() as usize;
    let by = (ATTACH_BTN_REL_Y * h as f64).round() as usize;

    for y in by - 5..=by + 5 {
        for x in ax - 10..=ax + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 10; pixels[idx + 1] = 10; pixels[idx + 2] = 120; pixels[idx + 3] = 255;
        }
        for x in rx - 10..=rx + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 10; pixels[idx + 1] = 10; pixels[idx + 2] = 120; pixels[idx + 3] = 255;
        }
    }

    assert!(find_red_button_in_pixels(&pixels, w, h, ATTACH_BTN_REL_X, ATTACH_BTN_REL_Y).is_some());
    assert!(find_red_button_in_pixels(&pixels, w, h, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y).is_some());

    // Scenario 2: Attached Mode (Normal View with "全部返还" at center, attach button GONE)
    // Clear attach button pixels:
    for y in by - 5..=by + 5 {
        for x in ax - 10..=ax + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 0; pixels[idx + 1] = 0; pixels[idx + 2] = 0; pixels[idx + 3] = 255;
        }
    }

    // Now attach button is GONE, but center button ("全部返还") is still present!
    assert_eq!(find_red_button_in_pixels(&pixels, w, h, ATTACH_BTN_REL_X, ATTACH_BTN_REL_Y), None);
    assert!(find_red_button_in_pixels(&pixels, w, h, ROTATE_BTN_REL_X, ROTATE_BTN_REL_Y).is_some());
}

#[test]
fn test_5th_board_limit_confirm_button_detection() {
    use paragon_clicker_rust::automation::board_connector::{
        find_red_button_in_pixels, CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y,
    };

    let w = 1024usize;
    let h = 576usize;
    let mut pixels = vec![0u8; w * h * 4];

    // Initially: modal is not present
    assert_eq!(
        find_red_button_in_pixels(&pixels, w, h, CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y),
        None
    );

    // Paint the 5th board warning dialog "确认" button at (0.460 * 1024 = 471, 0.597 * 576 = 344)
    let cx = (CONFIRM_BTN_REL_X * w as f64).round() as usize;
    let cy = (CONFIRM_BTN_REL_Y * h as f64).round() as usize;

    for y in cy - 5..=cy + 5 {
        for x in cx - 10..=cx + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 15;     // B
            pixels[idx + 1] = 15; // G
            pixels[idx + 2] = 130; // R
            pixels[idx + 3] = 255;
        }
    }

    // Modal confirm button should now be detected!
    let found = find_red_button_in_pixels(&pixels, w, h, CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y);
    assert!(found.is_some());
    let (fx, fy) = found.unwrap();
    assert_eq!(fx, cx as i32);
    assert_eq!(fy, cy as i32);

    // Clear the button (modal dismissed)
    for y in cy - 5..=cy + 5 {
        for x in cx - 10..=cx + 10 {
            let idx = (y * w + x) * 4;
            pixels[idx] = 0;
            pixels[idx + 1] = 0;
            pixels[idx + 2] = 0;
            pixels[idx + 3] = 255;
        }
    }
    assert_eq!(
        find_red_button_in_pixels(&pixels, w, h, CONFIRM_BTN_REL_X, CONFIRM_BTN_REL_Y),
        None
    );
}
