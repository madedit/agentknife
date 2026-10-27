//! Windows: WebView2's `CapturePreview`, which renders what the web view shows
//! into a PNG, also while the window is covered by others.

use std::cell::Cell;
use std::rc::Rc;

use webview2_com::CapturePreviewCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, ICoreWebView2Controller,
};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};

/// Called once with the PNG, or with what went wrong.
pub type Done = Box<dyn FnOnce(Result<Vec<u8>, String>)>;

/// Captures the web view of `controller` as a PNG, in the web view's own
/// pixels. Must run on the thread that owns the web view (Tauri's
/// `with_webview` closure); `done` is called later on that thread.
pub fn capture(controller: &ICoreWebView2Controller, done: Done) {
    // `done` goes to the completion handler, or is called here on an early error.
    let done = Rc::new(Cell::new(Some(done)));
    let finish = {
        let done = done.clone();
        move |result: Result<Vec<u8>, String>| {
            if let Some(f) = done.take() {
                f(result);
            }
        }
    };
    if let Err(e) = start(controller, finish.clone()) {
        finish(Err(format!("WebView2 could not capture the page: {e}")));
    }
}

fn start(
    controller: &ICoreWebView2Controller,
    finish: impl Fn(Result<Vec<u8>, String>) + 'static,
) -> windows::core::Result<()> {
    // SAFETY: a COM call on a live controller, on its own thread (see `capture`).
    let webview = unsafe { controller.CoreWebView2()? };
    // SAFETY: a new empty in-memory stream; it frees its memory when released.
    let stream: IStream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true)? };
    let filled = stream.clone();
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
        finish(
            result
                .and_then(|()| read_all(&filled))
                .map_err(|e| format!("WebView2 could not capture the page: {e}")),
        );
        Ok(())
    }));
    // SAFETY: COM call on the web view's thread; the stream and handler are
    // reference-counted COM objects that WebView2 holds until it calls back.
    unsafe {
        webview.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )
    }
}

/// Everything written to `stream`, from its start.
fn read_all(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut stat = STATSTG::default();
    // SAFETY: `stat` is a valid STATSTG to fill; STATFLAG_NONAME means no name
    // string is allocated (nothing to free).
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME)? };
    let len = usize::try_from(stat.cbSize).map_err(|_| windows::core::Error::empty())?;
    // SAFETY: plain COM call on a live stream.
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None)? };
    let mut bytes = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        let chunk = u32::try_from(len - filled).unwrap_or(u32::MAX);
        let mut read = 0u32;
        // SAFETY: the buffer has `len - filled` writable bytes from that
        // offset, and `chunk` is at most that; `read` is a valid out pointer.
        unsafe {
            stream
                .Read(bytes[filled..].as_mut_ptr().cast(), chunk, Some(&mut read))
                .ok()?;
        }
        if read == 0 {
            break;
        }
        filled += read as usize;
    }
    bytes.truncate(filled);
    Ok(bytes)
}
