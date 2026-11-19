//! Folders outside the controller's own which the host chooses, e.g. a library's, and keeping access to them. A
//! sandboxed app, as the Mac App Store's must be, may only read a folder the host chooses in the system's Open dialog,
//! and only until it quits, so each folder chosen is remembered by a security-scoped bookmark, which gives that access
//! again the next time the app runs. Where there is no sandbox, e.g. a build run from the command line, a bookmark
//! changes nothing, and a typed folder is read as it always was.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folders whose bookmarks have been opened while the app runs, so that each is opened once.
static RESUMED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Asks the host to choose a folder in the system's Open dialog, starting at `start` if it is a folder, and
/// remembers the folder chosen, returning it, or None if they cancelled.
pub async fn choose(title: &str, start: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(start) = start.filter(|start| start.is_dir()) {
        dialog = dialog.set_directory(start);
    }
    let folder = dialog.pick_folder().await?.path().to_path_buf();
    remember(&folder);
    Some(folder)
}

/// Gives access again to a folder chosen in an earlier run, from its bookmark, so that a sandboxed app can read it.
/// It does nothing for a folder with no bookmark, e.g. one typed, which a sandboxed app cannot read, or once access has
/// been given.
pub fn resume(folder: &Path) {
    let mut resumed = RESUMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if resumed.contains(folder) {
        return;
    }
    let mut bookmarks = read_bookmarks();
    let Some(bookmark) = bookmarks
        .get(&folder.display().to_string())
        .and_then(|hex| from_hex(hex))
    else {
        return;
    };
    if let Some(opened) = platform::open(&bookmark) {
        // A bookmark goes stale, e.g. when the folder is moved, and is made again, from where it is now.
        if opened.stale
            && let Some(fresh) = platform::bookmark(&opened.folder)
        {
            bookmarks.insert(opened.folder.display().to_string(), to_hex(&fresh));
            write_bookmarks(&bookmarks);
        }
        resumed.insert(folder.to_path_buf());
    }
}

/// Keeps a bookmark of a folder the host chose, replacing any it had.
fn remember(folder: &Path) {
    if let Some(bookmark) = platform::bookmark(folder) {
        let mut bookmarks = read_bookmarks();
        bookmarks.insert(folder.display().to_string(), to_hex(&bookmark));
        write_bookmarks(&bookmarks);
        RESUMED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(folder.to_path_buf());
    }
}

/// The bookmarks kept, each folder's in hex, in the controller's data folder.
fn bookmarks_file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bruce-quiz-party")
        .join("folder-bookmarks.json")
}

fn read_bookmarks() -> BTreeMap<String, String> {
    std::fs::read_to_string(bookmarks_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_bookmarks(bookmarks: &BTreeMap<String, String>) {
    let file = bookmarks_file();
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(text) = serde_json::to_string_pretty(bookmarks) {
        let _ = std::fs::write(file, text);
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
        .collect()
}

/// A bookmark opened: the folder it is of, as it is now, and whether it should be made again.
struct Opened {
    folder: PathBuf,
    stale: bool,
}

/// Security-scoped bookmarks, with Foundation's NSURL.
#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};

    use objc2::runtime::Bool;
    use objc2_foundation::{NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions};

    use super::Opened;

    /// A security-scoped bookmark of a folder, or None if one cannot be made.
    pub fn bookmark(folder: &Path) -> Option<Vec<u8>> {
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(folder.to_str()?), true);
        url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::WithSecurityScope,
            None,
            None,
        )
        .ok()
        .map(|data| data.to_vec())
    }

    /// Opens a bookmark and starts accessing its folder, which is kept until the app quits, as the folder is read
    /// whenever the host looks at it.
    pub fn open(bookmark: &[u8]) -> Option<Opened> {
        let data = NSData::with_bytes(bookmark);
        let mut stale = Bool::NO;
        // SAFETY: stale is a valid pointer to a Bool for the length of the call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope,
                None,
                &mut stale,
            )
        }
        .ok()?;
        // SAFETY: the URL is a file URL from a security-scoped bookmark, as the method expects.
        unsafe { url.startAccessingSecurityScopedResource() };
        Some(Opened {
            folder: PathBuf::from(url.path()?.to_string()),
            stale: stale.as_bool(),
        })
    }
}

/// Elsewhere there is no sandbox, so a folder needs no bookmark.
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use super::Opened;

    pub fn bookmark(_folder: &Path) -> Option<Vec<u8>> {
        None
    }

    pub fn open(_bookmark: &[u8]) -> Option<Opened> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_goes_both_ways() {
        let bytes = [0u8, 1, 0x7f, 0xff];
        assert_eq!(to_hex(&bytes), "00017fff");
        assert_eq!(from_hex("00017fff").unwrap(), bytes);
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }
}
