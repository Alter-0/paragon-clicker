use paragon_clicker_rust::automation::vision::detect_paragon_board;

#[test]
fn test_detect_paragon_board_synthetic_single() {
    let width = 1024;
    let height = 600;
    let mut pixels = vec![10u8; width * height * 4]; // Dark background (BGRA)

    let expected_left = 318;
    let expected_top = 96;
    let expected_right = 700;
    let expected_bottom = 478;

    // Draw red border lines (BGRA: B=15, G=15, R=90, A=255)
    for x in expected_left..=expected_right {
        // Top line
        let idx_top = (expected_top * width + x) * 4;
        pixels[idx_top] = 15;
        pixels[idx_top + 1] = 15;
        pixels[idx_top + 2] = 90;

        // Bottom line
        let idx_bot = (expected_bottom * width + x) * 4;
        pixels[idx_bot] = 15;
        pixels[idx_bot + 1] = 15;
        pixels[idx_bot + 2] = 90;
    }

    for y in expected_top..=expected_bottom {
        // Left line
        let idx_left = (y * width + expected_left) * 4;
        pixels[idx_left] = 15;
        pixels[idx_left + 1] = 15;
        pixels[idx_left + 2] = 90;

        // Right line
        let idx_right = (y * width + expected_right) * 4;
        pixels[idx_right] = 15;
        pixels[idx_right + 1] = 15;
        pixels[idx_right + 2] = 90;
    }

    let detected = detect_paragon_board(&pixels, width, height)
        .expect("Should detect synthetic board");

    assert_eq!(detected.left, expected_left as i32);
    assert_eq!(detected.top, expected_top as i32);
    assert_eq!(detected.right, expected_right as i32);
    assert_eq!(detected.bottom, expected_bottom as i32);
    assert_eq!(detected.width, (expected_right - expected_left) as i32);
    assert_eq!(detected.height, (expected_bottom - expected_top) as i32);
}

#[test]
fn test_detect_paragon_board_dual_board_picks_center() {
    let width = 1200;
    let height = 700;
    let mut pixels = vec![10u8; width * height * 4];

    // Left board: [200..584], [100..484] -> center (392, 292)
    // Right board: [584..968], [100..484] -> center (776, 292)
    // Target center is (0.6 * 1200, 0.5 * 700) = (720, 350)
    // Right board is closer to (720, 350)

    let draw_box = |pixels: &mut [u8], l: usize, t: usize, r: usize, b: usize| {
        for x in l..=r {
            for &y in &[t, b] {
                let idx = (y * width + x) * 4;
                pixels[idx] = 10;
                pixels[idx + 1] = 10;
                pixels[idx + 2] = 85;
            }
        }
        for y in t..=b {
            for &x in &[l, r] {
                let idx = (y * width + x) * 4;
                pixels[idx] = 10;
                pixels[idx + 1] = 10;
                pixels[idx + 2] = 85;
            }
        }
    };

    draw_box(&mut pixels, 200, 100, 584, 484);
    draw_box(&mut pixels, 584, 100, 968, 484);

    let detected = detect_paragon_board(&pixels, width, height)
        .expect("Should detect board");

    // Must choose right board because it's centered in the viewport
    assert_eq!(detected.left, 584);
    assert_eq!(detected.right, 968);
    assert_eq!(detected.top, 100);
    assert_eq!(detected.bottom, 484);
}

#[test]
fn test_detect_paragon_board_rejects_non_square() {
    let width = 1000;
    let height = 600;
    let mut pixels = vec![10u8; width * height * 4];

    // Draw a rectangle (width 400, height 200) - not 1:1 square
    let l = 300; let r = 700;
    let t = 200; let b = 400;
    for x in l..=r {
        for &y in &[t, b] {
            let idx = (y * width + x) * 4;
            pixels[idx] = 10;
            pixels[idx + 1] = 10;
            pixels[idx + 2] = 100;
        }
    }
    for y in t..=b {
        for &x in &[l, r] {
            let idx = (y * width + x) * 4;
            pixels[idx] = 10;
            pixels[idx + 1] = 10;
            pixels[idx + 2] = 100;
        }
    }

    let detected = detect_paragon_board(&pixels, width, height);
    assert!(detected.is_none(), "Rectangles that are not 1:1 squares must be rejected");
}
