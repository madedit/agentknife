//! Screenshots of the preview (`screenshot_preview`): AgentKnife's window
//! captures its own web view (ak-capture) and cuts out the part where the
//! preview page shows, which the window reports with `set_preview_frame`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_capture::PixelRect;
use ak_preview::{Capturer, Image};
use ak_types::preview::{PreviewFrame, ScreenshotArea, Size};
use base64::Engine;
use futures_util::future::BoxFuture;
use tauri::{AppHandle, Manager, Runtime};

use crate::state::AppState;

/// How long the platform may take to capture the window.
const CAPTURE_WAIT: Duration = Duration::from_secs(10);
const NOT_SHOWN: &str = "The preview is not on screen in AgentKnife (another tab is open \
                         there, or the window is minimized), so there is nothing to \
                         photograph. Use dom_summary instead, or ask the user to show the \
                         preview.";

/// The preview's screenshots, where this platform's web view can be captured.
pub fn capturer<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> Option<Arc<dyn Capturer>> {
    ak_capture::AVAILABLE.then(|| {
        Arc::new(WindowCapturer {
            app: app.clone(),
            frame: state.frame.clone(),
        }) as Arc<dyn Capturer>
    })
}

struct WindowCapturer<R: Runtime> {
    app: AppHandle<R>,
    frame: Arc<Mutex<Option<PreviewFrame>>>,
}

impl<R: Runtime> Capturer for WindowCapturer<R> {
    fn page_size(&self) -> Option<Size> {
        let frame = self.frame.lock().ok().and_then(|f| *f)?;
        (frame.scale > 0.0).then(|| Size {
            width: frame.rect.width / frame.scale,
            height: frame.rect.height / frame.scale,
        })
    }

    fn capture<'a>(&'a self, area: &'a ScreenshotArea) -> BoxFuture<'a, Result<Image, String>> {
        Box::pin(async move {
            let frame = self.frame.lock().ok().and_then(|f| *f).ok_or(NOT_SHOWN)?;
            let png = self.window_png().await?;
            let (width, _) = ak_capture::size(&png)?;
            let rect = crop_rect(&frame, area, width).ok_or(NOT_SHOWN)?;
            let cut = ak_capture::crop(&png, rect)?;
            Ok(Image {
                media_type: "image/png".into(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(cut),
            })
        })
    }
}

impl<R: Runtime> WindowCapturer<R> {
    /// The whole window's web view as a PNG.
    async fn window_png(&self) -> Result<Vec<u8>, String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or("AgentKnife's window is not open.")?;
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
        #[cfg(windows)]
        window
            .with_webview(move |webview| {
                ak_capture::webview2::capture(
                    &webview.controller(),
                    Box::new(move |png| {
                        let _ = tx.send(png);
                    }),
                );
            })
            .map_err(|e| e.to_string())?;
        #[cfg(not(windows))]
        drop((window, tx));
        match tokio::time::timeout(CAPTURE_WAIT, rx).await {
            Ok(Ok(png)) => png,
            Ok(Err(_)) => Err("The window could not be captured.".into()),
            Err(_) => Err("The window took too long to capture.".into()),
        }
    }
}

/// The capture's pixels showing `area` of the page: `area` is in the page's
/// CSS pixels, clipped to what the frame shows; the capture is
/// `image_width` pixels wide for the window's CSS width. `None` when nothing
/// of it shows.
fn crop_rect(frame: &PreviewFrame, area: &ScreenshotArea, image_width: u32) -> Option<PixelRect> {
    if frame.scale <= 0.0 || frame.window_width <= 0.0 {
        return None;
    }
    let shown_w = frame.rect.width / frame.scale;
    let shown_h = frame.rect.height / frame.scale;
    let x0 = area.rect.x.clamp(0.0, shown_w);
    let y0 = area.rect.y.clamp(0.0, shown_h);
    let x1 = (area.rect.x + area.rect.width).clamp(0.0, shown_w);
    let y1 = (area.rect.y + area.rect.height).clamp(0.0, shown_h);
    let px = f64::from(image_width) / frame.window_width;
    let to_px = |v: f64| (v * px).round().max(0.0) as u32;
    let left = to_px(frame.rect.x + x0 * frame.scale);
    let top = to_px(frame.rect.y + y0 * frame.scale);
    let right = to_px(frame.rect.x + x1 * frame.scale);
    let bottom = to_px(frame.rect.y + y1 * frame.scale);
    (right > left && bottom > top).then_some(PixelRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

#[cfg(test)]
mod tests {
    use ak_types::preview::Rect;

    use super::*;

    fn area(x: f64, y: f64, width: f64, height: f64) -> ScreenshotArea {
        ScreenshotArea {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            viewport: Size {
                width: 800.0,
                height: 600.0,
            },
            device_pixel_ratio: 1.5,
        }
    }

    #[test]
    fn the_page_area_becomes_the_windows_pixels() {
        // A 800×600 page at (300, 40) in a 1200-wide window, captured at 1.5×.
        let frame = PreviewFrame {
            rect: Rect {
                x: 300.0,
                y: 40.0,
                width: 800.0,
                height: 600.0,
            },
            scale: 1.0,
            window_width: 1200.0,
        };
        let whole = crop_rect(&frame, &area(0.0, 0.0, 800.0, 600.0), 1800).unwrap();
        assert_eq!(
            whole,
            PixelRect {
                x: 450,
                y: 60,
                width: 1200,
                height: 900
            }
        );
        // An element partly below the frame: only what shows.
        let low = crop_rect(&frame, &area(100.0, 500.0, 50.0, 300.0), 1200).unwrap();
        assert_eq!(
            low,
            PixelRect {
                x: 400,
                y: 540,
                width: 50,
                height: 100
            }
        );
        assert_eq!(crop_rect(&frame, &area(0.0, 700.0, 10.0, 10.0), 1200), None);
    }

    #[test]
    fn a_scaled_phone_frame_is_scaled_too() {
        // A 390-wide phone page shown at half size.
        let frame = PreviewFrame {
            rect: Rect {
                x: 500.0,
                y: 12.0,
                width: 195.0,
                height: 422.0,
            },
            scale: 0.5,
            window_width: 1000.0,
        };
        let r = crop_rect(&frame, &area(0.0, 0.0, 390.0, 844.0), 1000).unwrap();
        assert_eq!(
            r,
            PixelRect {
                x: 500,
                y: 12,
                width: 195,
                height: 422
            }
        );
    }
}
