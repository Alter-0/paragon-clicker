use paragon_clicker_rust::automation::recognize_text_from_rgba;

#[test]
fn test_ocr_empty_input() {
    let res = recognize_text_from_rgba(0, 0, &[]);
    assert_eq!(res.unwrap(), "");
}

#[test]
fn test_ocr_dummy_image() {
    // 50x50 black image with some white pixels
    let w = 60u32;
    let h = 30u32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        pixels[i * 4] = 255;
        pixels[i * 4 + 1] = 255;
        pixels[i * 4 + 2] = 255;
        pixels[i * 4 + 3] = 255;
    }
    let res = recognize_text_from_rgba(w, h, &pixels);
    assert!(res.is_ok());
}
