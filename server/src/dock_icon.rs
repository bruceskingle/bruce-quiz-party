//! The app's icon in the Dock, set as it starts. A bundle made by `dx bundle` has the icon already, but the app `dx
//! serve` runs names it without including it, and `cargo run` has no bundle at all, so without this the Dock shows
//! a placeholder while working on the app.

/// The icon, as the bundle has it, made by art/make-icons.sh.
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/icon.icns");

/// Sets the Dock's icon. It must be called on the main thread once the application exists, as a component's hook is,
/// since making the application here first would stop the window library from making its own.
#[cfg(target_os = "macos")]
pub fn set() {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(main_thread);
        // SAFETY: the image is a valid NSImage, and this runs on the main thread, as AppKit requires.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

/// Elsewhere there is no Dock to set.
#[cfg(not(target_os = "macos"))]
pub fn set() {}
