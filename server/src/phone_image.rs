//! Smaller copies of quizzes' photos, for the phones. A photo from Wikimedia Commons may be several megabytes, far more
//! than a phone's screen shows; with many phones fetching it at once over Wi-Fi, some would wait seconds for it. The
//! copies are made with macOS's own `sips`, so no image library is needed, and kept in the app's cache folder, named
//! for the original and its size and time, so that a changed image is copied again. SVG and GIF images are sent as
//! they are, being small, or animated.

use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex},
    time::UNIX_EPOCH,
};

use anyhow::Context;

/// The longest side of a copy, in pixels: enough for a phone's screen at its full resolution.
pub const MAX_SIDE: u32 = 1280;

/// Images no bigger than this are sent as they are.
const SMALL_ENOUGH: u64 = 300 * 1024;

/// The images being copied, so that each is copied once, however many phones ask for it at the same time.
static COPYING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// The file to send a phone for an image: its smaller copy, if one has been made, else the image itself, whose copy is
/// then started, for the next phone to ask.
pub fn for_phone(original: &Path) -> PathBuf {
    match copy_path(original) {
        Some(copy) if copy.is_file() => copy,
        Some(_) => {
            prepare(vec![original.to_path_buf()]);
            original.to_path_buf()
        }
        None => original.to_path_buf(),
    }
}

/// Makes the smaller copies of images which need them, on a thread of its own, e.g. as a party starts, so that they
/// are ready before the questions are asked.
pub fn prepare(originals: Vec<PathBuf>) {
    let originals: Vec<PathBuf> = {
        let mut copying = COPYING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        originals
            .into_iter()
            .filter(|original| copy_path(original).is_some_and(|copy| !copy.is_file()))
            .filter(|original| copying.insert(original.clone()))
            .collect()
    };
    if originals.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for original in originals {
            if let Some(copy) = copy_path(&original)
                && let Err(error) = shrink(&original, &copy)
            {
                eprintln!("Cannot make a smaller copy of {}: {error:#}", original.display());
            }
            COPYING
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&original);
        }
    });
}

/// Where an image's smaller copy is kept, or None if it is sent as it is: an SVG or GIF, or small enough already.
fn copy_path(original: &Path) -> Option<PathBuf> {
    let extension = original.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp") {
        return None;
    }
    let metadata = std::fs::metadata(original).ok()?;
    if metadata.len() <= SMALL_ENOUGH {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    original.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .hash(&mut hasher);
    MAX_SIDE.hash(&mut hasher);
    // A WebP is copied as a JPEG, which sips can write.
    let extension = if extension == "webp" {
        "jpg".to_string()
    } else {
        extension
    };
    Some(cache_folder()?.join(format!("{:016x}.{extension}", hasher.finish())))
}

/// The folder the copies are kept in.
fn cache_folder() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("bruce-quiz-party").join("phone-images"))
}

/// Makes a smaller copy of an image, at most [`MAX_SIDE`] on its longest side, a JPEG re-compressed; if that is no
/// smaller, the copy is the original, so that it is not looked at again.
fn shrink(original: &Path, copy: &Path) -> anyhow::Result<()> {
    if let Some(folder) = copy.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let (width, height) = size_of(original)?;
    let partial = copy.with_extension(format!(
        "partial.{}",
        copy.extension().and_then(|e| e.to_str()).unwrap_or("jpg")
    ));
    let mut sips = Command::new("sips");
    if width.max(height) > MAX_SIDE {
        sips.arg("--resampleHeightWidthMax").arg(MAX_SIDE.to_string());
    }
    // The copy is written in the format its name says, a JPEG fairly compressed, as a phone's screen shows no more.
    match copy.extension().and_then(|e| e.to_str()) {
        Some("png") => sips.args(["--setProperty", "format", "png"]),
        _ => sips.args([
            "--setProperty",
            "format",
            "jpeg",
            "--setProperty",
            "formatOptions",
            "80",
        ]),
    };
    let output = sips
        .arg(original)
        .arg("--out")
        .arg(&partial)
        .output()
        .context("cannot run sips")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&partial);
        anyhow::bail!("sips failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    // A copy no smaller, e.g. of a PNG already small enough to fit, is the original itself, kept so that it is not
    // looked at again; unless it is in another format, which the phones may need.
    let same_format = original.extension().map(|e| e.to_ascii_lowercase()) == copy.extension().map(|e| e.into());
    let size = |path: &Path| {
        std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(u64::MAX)
    };
    if same_format && size(&partial) >= size(original) {
        std::fs::copy(original, &partial).context("cannot copy the original")?;
    }
    // Only a finished copy has the copy's name, so that a phone is never sent half of one.
    std::fs::rename(&partial, copy).context("cannot keep the copy")?;
    Ok(())
}

/// An image's width and height in pixels, as sips reads them.
fn size_of(image: &Path) -> anyhow::Result<(u32, u32)> {
    let output = Command::new("sips")
        .args(["--getProperty", "pixelWidth", "--getProperty", "pixelHeight"])
        .arg(image)
        .output()
        .context("cannot run sips")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(name)?.trim().parse::<u32>().ok())
    };
    match (value("pixelWidth:"), value("pixelHeight:")) {
        (Some(width), Some(height)) => Ok((width, height)),
        _ => anyhow::bail!("sips cannot read its size"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BMP of the given size, all one colour, which sips reads; written by hand, as there is no image library.
    fn bmp(width: u32, height: u32) -> Vec<u8> {
        let row = (width * 3).div_ceil(4) * 4;
        let pixels = row * height;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&(54 + pixels).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&(width as i32).to_le_bytes());
        bytes.extend_from_slice(&(height as i32).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 24]);
        bytes.resize(bytes.len() + pixels as usize, 0x80);
        bytes
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn shrinks_a_big_photo_to_fit_a_phone() {
        let folder = std::env::temp_dir().join(format!("phone-image-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let original = folder.join("big.bmp");
        std::fs::write(&original, bmp(3000, 1500)).unwrap();
        let copy = folder.join("copy.png");
        shrink(&original, &copy).unwrap();
        assert_eq!(size_of(&copy).unwrap(), (MAX_SIDE, MAX_SIDE / 2));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn keeps_the_original_if_a_copy_is_no_smaller() {
        let folder = std::env::temp_dir().join(format!("phone-image-same-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let bitmap = folder.join("small.bmp");
        std::fs::write(&bitmap, bmp(400, 300)).unwrap();
        let original = folder.join("small.png");
        let made = Command::new("sips")
            .args(["--setProperty", "format", "png"])
            .arg(&bitmap)
            .arg("--out")
            .arg(&original)
            .output()
            .unwrap();
        assert!(made.status.success());
        let copy = folder.join("copy.png");
        shrink(&original, &copy).unwrap();
        assert!(std::fs::metadata(&copy).unwrap().len() <= std::fs::metadata(&original).unwrap().len());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn sends_small_and_drawn_images_as_they_are() {
        let folder = std::env::temp_dir().join(format!("phone-image-small-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let small = folder.join("small.png");
        std::fs::write(&small, [0u8; 1000]).unwrap();
        let drawing = folder.join("flag.svg");
        std::fs::write(&drawing, vec![b' '; 2_000_000]).unwrap();
        assert_eq!(copy_path(&small), None);
        assert_eq!(copy_path(&drawing), None);
        assert_eq!(for_phone(&drawing), drawing);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
