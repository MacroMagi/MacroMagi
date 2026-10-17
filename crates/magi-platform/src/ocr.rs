//! Reading text in pictures with the text recognition built into Windows
//! (`Windows.Media.Ocr`). It needs no download: Windows recognises the
//! languages that have their "optical character recognition" feature
//! installed, which the display language usually has.

use magi_core::host::{Frame, HostError, TextWord};

#[cfg(windows)]
pub fn read_text(picture: &Frame, language: &str) -> Result<Vec<TextWord>, HostError> {
    use magi_core::geom::Rect;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::core::HSTRING;

    let failed = |e: windows::core::Error| HostError::new(format!("text recognition failed: {}", e.message()));
    let language = language.trim();
    let engine = if language.is_empty() {
        OcrEngine::TryCreateFromUserProfileLanguages()
    } else {
        Language::CreateLanguage(&HSTRING::from(language)).and_then(|tag| OcrEngine::TryCreateFromLanguage(&tag))
    }
    .map_err(|_| {
        HostError::new(format!(
            "Windows cannot read {}. Add the language in Settings › Time & language › Language & region, with \
             its optical character recognition feature.",
            if language.is_empty() { "text in your languages".to_owned() } else { format!("“{language}”") }
        ))
    })?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    if picture.width > max || picture.height > max {
        return Err(HostError::new(format!("the picture is too large to read (at most {max} pixels a side)")));
    }
    // Windows wants blue, green, red, alpha.
    let mut bgra = picture.rgba.clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let writer = DataWriter::new().map_err(failed)?;
    writer.WriteBytes(&bgra).map_err(failed)?;
    let buffer = writer.DetachBuffer().map_err(failed)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        picture.width as i32,
        picture.height as i32,
    )
    .map_err(failed)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(failed)?.join().map_err(failed)?;
    let mut words = Vec::new();
    for (line, found) in result.Lines().map_err(failed)?.into_iter().enumerate() {
        for word in found.Words().map_err(failed)? {
            let bounds = word.BoundingRect().map_err(failed)?;
            words.push(TextWord {
                text: word.Text().map_err(failed)?.to_string(),
                rect: Rect::new(
                    bounds.X.round() as i32,
                    bounds.Y.round() as i32,
                    bounds.Width.round().max(1.0) as u32,
                    bounds.Height.round().max(1.0) as u32,
                ),
                line,
            });
        }
    }
    Ok(words)
}

#[cfg(not(windows))]
pub fn read_text(picture: &Frame, language: &str) -> Result<Vec<TextWord>, HostError> {
    let _ = (picture, language);
    Err(HostError::new(
        "Text Scan uses the text recognition built into Windows; it is not available on this system yet",
    ))
}
