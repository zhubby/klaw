use anyhow::Context;

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/logo.iconset/icon_512x512@2x.png");

#[cfg(target_os = "macos")]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../assets/icons/logo.iconset/status_bar_icon.png");

#[cfg(not(target_os = "macos"))]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../assets/icons/logo.iconset/status_bar_icon.png");

struct DecodedIcon {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

pub fn viewport_icon() -> Option<egui::IconData> {
    let decoded = decode_png_icon(APP_ICON_PNG).ok()?;
    Some(egui::IconData {
        rgba: decoded.rgba,
        width: decoded.width,
        height: decoded.height,
    })
}

pub fn tray_icon() -> anyhow::Result<tray_icon::Icon> {
    let decoded = decode_png_icon(TRAY_ICON_PNG)?;
    tray_icon::Icon::from_rgba(decoded.rgba, decoded.width, decoded.height)
        .map_err(|err| anyhow::anyhow!("failed to convert embedded tray icon: {err}"))
}

#[cfg(target_os = "macos")]
pub fn application_icon_image() -> Option<objc2::rc::Retained<objc2_app_kit::NSImage>> {
    use objc2::ClassType as _;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSData;

    let data = NSData::with_bytes(APP_ICON_PNG);
    NSImage::initWithData(NSImage::alloc(), &data)
}

fn decode_png_icon(bytes: &[u8]) -> anyhow::Result<DecodedIcon> {
    let image = image::load_from_memory(bytes)
        .context("failed to decode embedded icon image")?
        .into_rgba8();
    Ok(DecodedIcon {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON_PNG, TRAY_ICON_PNG, decode_png_icon};

    #[test]
    fn embedded_viewport_icon_decodes() {
        let decoded = decode_png_icon(APP_ICON_PNG).expect("app icon should decode");
        assert!(decoded.width > 0);
        assert!(decoded.height > 0);
        assert_eq!(
            decoded.rgba.len(),
            decoded.width as usize * decoded.height as usize * 4
        );
    }

    #[test]
    fn embedded_tray_icon_decodes() {
        let decoded = decode_png_icon(TRAY_ICON_PNG).expect("tray icon should decode");
        assert!(decoded.width > 0);
        assert!(decoded.height > 0);
        assert_eq!(
            decoded.rgba.len(),
            decoded.width as usize * decoded.height as usize * 4
        );
    }
}
