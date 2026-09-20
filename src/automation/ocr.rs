use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

/// Encodes raw RGBA8 pixels into an uncompressed BMP byte buffer in memory.
pub fn encode_bmp_rgba(width: u32, height: u32, rgba_pixels: &[u8]) -> Vec<u8> {
    let row_size = ((width * 3 + 3) / 4) * 4; // 24-bit BGR with 4-byte row alignment
    let image_size = row_size * height;
    let file_size = 54 + image_size;

    let mut bmp = Vec::with_capacity(file_size as usize);

    // BITMAPFILEHEADER (14 bytes)
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]); // reserved
    bmp.extend_from_slice(&54u32.to_le_bytes()); // offset to pixel data

    // BITMAPINFOHEADER (40 bytes)
    bmp.extend_from_slice(&40u32.to_le_bytes()); // header size
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes()); // bottom-up
    bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
    bmp.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    bmp.extend_from_slice(&0u32.to_le_bytes()); // compression (BI_RGB)
    bmp.extend_from_slice(&image_size.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes()); // colors used
    bmp.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data (BGR bottom-up)
    let pad = vec![0u8; (row_size - width * 3) as usize];
    for y in (0..height).rev() {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if idx + 2 < rgba_pixels.len() {
                let r = rgba_pixels[idx];
                let g = rgba_pixels[idx + 1];
                let b = rgba_pixels[idx + 2];
                bmp.push(b);
                bmp.push(g);
                bmp.push(r);
            } else {
                bmp.extend_from_slice(&[0, 0, 0]);
            }
        }
        bmp.extend_from_slice(&pad);
    }

    bmp
}

/// Recognizes text from raw RGBA8 image buffer using Windows Native OCR (`Windows.Media.Ocr`).
pub fn recognize_text_from_rgba(width: u32, height: u32, rgba_pixels: &[u8]) -> Result<String, String> {
    if width == 0 || height == 0 || rgba_pixels.is_empty() {
        return Ok(String::new());
    }

    // Windows OCR works best when image has sufficient resolution (scale small clips)
    let (w, h, pixels) = if width < 120 || height < 40 {
        let scale = 2u32;
        let nw = width * scale;
        let nh = height * scale;
        let mut scaled = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            let src_y = y / scale;
            for x in 0..nw {
                let src_x = x / scale;
                let src_idx = ((src_y * width + src_x) * 4) as usize;
                let dst_idx = ((y * nw + x) * 4) as usize;
                scaled[dst_idx..dst_idx + 4].copy_from_slice(&rgba_pixels[src_idx..src_idx + 4]);
            }
        }
        (nw, nh, scaled)
    } else {
        (width, height, rgba_pixels.to_vec())
    };

    let bmp_bytes = encode_bmp_rgba(w, h, &pixels);

    let stream = InMemoryRandomAccessStream::new().map_err(|e| format!("Create stream error: {e}"))?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(|e| format!("Create writer error: {e}"))?;
    writer.WriteBytes(&bmp_bytes).map_err(|e| format!("Write bytes error: {e}"))?;
    writer.StoreAsync().map_err(|e| format!("StoreAsync error: {e}"))?.join().map_err(|e| format!("Store join error: {e}"))?;
    writer.DetachStream().map_err(|e| format!("Detach error: {e}"))?;
    stream.Seek(0).map_err(|e| format!("Seek error: {e}"))?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| format!("Create decoder error: {e}"))?
        .join()
        .map_err(|e| format!("Decoder join error: {e}"))?;

    let bitmap = decoder.GetSoftwareBitmapAsync()
        .map_err(|e| format!("GetSoftwareBitmap error: {e}"))?
        .join()
        .map_err(|e| format!("Bitmap join error: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("TryCreateFromUserProfileLanguages error: {e}"))?;

    let ocr_result = engine.RecognizeAsync(&bitmap)
        .map_err(|e| format!("RecognizeAsync error: {e}"))?
        .join()
        .map_err(|e| format!("Recognize join error: {e}"))?;

    let text = ocr_result.Text().map_err(|e| format!("Get text error: {e}"))?;
    Ok(text.to_string())
}
