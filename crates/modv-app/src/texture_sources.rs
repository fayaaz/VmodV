//! Texture-prop resolution: image assets and the request parsing shared by the
//! ISF render paths.
//!
//! The legacy worker resolves a `texture` prop through
//! `dataTypes.texture.get` (`store/modules/dataTypes.js:96-103`) into whatever
//! object the prop's `location` names, and the ISF renderer binds that object's
//! `.value` (`renderers/isf/index.js:84-91`); an unassigned **or unresolvable**
//! prop binds the group canvas. The engine has no JS objects between the store
//! and the GPU, so this module supplies the two pieces that lookup needs:
//!
//! * [`TextureRequest`] — a pure parse of the saved prop value
//!   (`{ type, location, id, options: { id, path } }`, a bare string, or a
//!   resolved `{ value }` wrapper) into the ids and paths the compositor can
//!   match against live GPU textures.
//! * [`ImageTextures`] — a bounded cache of decoded image assets keyed by
//!   `(path, requested size)` and uploaded as `wgpu::Texture`s.
//!
//! # Image paths
//!
//! The media library stores image paths relative to the project media
//! directory (`default/image/name.png`), which Electron resolves as
//! `<userData>/media`. The engine is not told that root over the control
//! socket, so [`ImageTextures`] resolves it in this order:
//!
//! 1. an explicitly configured root ([`ImageTextures::set_media_root`], used by
//!    tests and available to a future `media/*` IPC),
//! 2. `$MODV_MEDIA_DIR`,
//! 3. Electron's userData default for the app name `VmodV`
//!    (`~/.config/VmodV/media` on Linux, the platform equivalents elsewhere).
//!
//! A truly absolute path that exists is used as-is, so an external image can be
//! referenced directly. Paths containing `..` are rejected: a Scene document is
//! shareable data and must not be able to read arbitrary files.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use modv_render::wgpu;

/// Upper bound on a decoded image (8192², the same guard the media textures
/// use). Larger images are refused rather than reserving gigabytes.
const MAX_IMAGE_PIXELS: u64 = 8192 * 8192;

/// Default bounded cache size for decoded image textures.
const DEFAULT_MAX_ENTRIES: usize = 16;

/// The texture-prop kinds that can name an image asset
/// (`dataTypes.js:42-55`).
const IMAGE_LOCATION: &str = "images/image";

/// Video-file textures (`dataTypes.js:57-70` / `videos/video`).
const VIDEO_LOCATION: &str = "videos/video";

/// Canvas/group textures (`dataTypes.js:72-76` / `outputs/auxillaryCanvas`).
const CANVAS_LOCATION: &str = "outputs/auxillaryCanvas";

/// Normalise a media-library path for comparison: `/` separators, no leading
/// separators.
///
/// Returns `None` for an empty path, a path with a `..` segment, or one with a
/// NUL byte. This is the guard shared by the image resolver and the media
/// source-id suffix match.
pub fn normalise_library_path(raw: &str) -> Option<String> {
    if raw.contains('\0') {
        return None;
    }
    let normalised = raw.trim().replace('\\', "/");
    if normalised.split('/').any(|segment| segment == "..") {
        return None;
    }
    let trimmed = normalised.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// What a saved texture prop asks the renderer for.
///
/// Only the parts the engine can act on are kept: the declared kind, the
/// legacy `location`, and the candidate ids/paths. The JS `.value` object
/// (an `ImageBitmap`, `<video>` or canvas) does not exist in the engine; a
/// saved primitive `value` string is treated as an id/path candidate instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureRequest {
    /// Lowercased texture `type`: `inherit`, `group`, `canvas`, `buffer`,
    /// `webcam`, `screen`, `image`, `video`, or an empty string when a bare
    /// string value was supplied.
    pub kind: String,
    /// The legacy `location` (`images/image`, `videos/video`,
    /// `outputs/auxillaryCanvas`), lowercased.
    pub location: String,
    /// Candidate texture ids: a group id, a module id, or a live media source
    /// id (`camera:…`, `video:…`, `screen:…`).
    pub ids: Vec<String>,
    /// Candidate image paths (media-library relative or absolute).
    pub paths: Vec<String>,
}

impl TextureRequest {
    /// Parse a saved texture prop value.
    ///
    /// Returns `None` for an unassigned prop (`null`/`undefined`) or an empty
    /// string, which is the JS canvas default. A `type` wins over `location`
    /// when both are present (the UI always writes `type`); `location` is what
    /// older presets and the legacy `dataTypes.create` output carry.
    pub fn from_value(value: Option<&Value>) -> Option<Self> {
        match value? {
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                Some(Self {
                    kind: String::new(),
                    location: String::new(),
                    ids: vec![text.to_string()],
                    paths: vec![text.to_string()],
                })
            }
            Value::Object(map) => {
                let declared = map.get("type").and_then(Value::as_str);
                let location = map
                    .get("location")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                let kind = declared
                    .map(|value| value.trim().to_lowercase())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| kind_from_location(&location));

                let mut ids = Vec::new();
                let mut paths = Vec::new();

                let mut push_id = |candidate: Option<&str>| {
                    let Some(candidate) = candidate
                        .map(str::trim)
                        .filter(|candidate| !candidate.is_empty())
                    else {
                        return;
                    };
                    if !ids.iter().any(|id| id == candidate) {
                        ids.push(candidate.to_string());
                    }
                };
                let mut push_path = |candidate: Option<&str>| {
                    let Some(candidate) = candidate
                        .map(str::trim)
                        .filter(|candidate| !candidate.is_empty())
                    else {
                        return;
                    };
                    if !paths.iter().any(|path| path == candidate) {
                        paths.push(candidate.to_string());
                    }
                };

                push_id(map.get("id").and_then(Value::as_str));
                let options = map.get("options");
                push_id(
                    options
                        .and_then(|options| options.get("id"))
                        .and_then(Value::as_str),
                );
                push_path(
                    options
                        .and_then(|options| options.get("path"))
                        .and_then(Value::as_str),
                );
                // A resolved `{ value }` wrapper: the engine sees a saved
                // primitive, never the JS texture object.
                if let Some(Value::String(resolved)) = map.get("value") {
                    push_id(Some(resolved));
                    push_path(Some(resolved));
                }

                if ids.is_empty() && paths.is_empty() && kind.is_empty() {
                    return None;
                }

                Some(Self {
                    kind,
                    location,
                    ids,
                    paths,
                })
            }
            // Numbers, bools and arrays are not texture definitions.
            _ => None,
        }
    }

    /// Whether the prop explicitly asks for an image asset.
    pub fn wants_image(&self) -> bool {
        self.kind == "image" || self.location == IMAGE_LOCATION
    }

    /// Candidate image paths, in preference order.
    ///
    /// An `image` prop may name any decodable file; every other kind only
    /// offers paths that look like images, so a `video`/`canvas` prop with an
    /// `options.path` is never fed to the image decoder (and never warns
    /// "could not decode the image" for a video file).
    pub fn image_paths(&self) -> Vec<&str> {
        self.paths
            .iter()
            .filter(|path| self.wants_image() || has_image_extension(path))
            .map(String::as_str)
            .collect()
    }

    /// The first image path the prop names, if any (preview fast path).
    pub fn first_image_path(&self) -> Option<&str> {
        self.image_paths().into_iter().next()
    }

    /// Candidate media source ids derived from the prop's ids.
    ///
    /// The renderer names engine sources with a kind prefix
    /// (`camera:<deviceId>`, `video:<path>`, `screen:<nodeId>`) in
    /// `engine-media.js`, and NDI receivers as `ndi:<receiverId>` (WS-11); a
    /// texture prop may carry either the bare key or the prefixed source id, so
    /// both spellings are tried.
    pub fn media_ids(&self) -> Vec<String> {
        let mut candidates: Vec<String> = Vec::with_capacity(self.ids.len() * 5);
        let mut push = |candidate: String| {
            if !candidates.iter().any(|existing| existing == &candidate) {
                candidates.push(candidate);
            }
        };
        for id in &self.ids {
            push(id.clone());
            if let Some((_kind, key)) = id.split_once(':').filter(|(_, key)| !key.is_empty()) {
                push(key.to_string());
            }
            for prefix in ["video:", "camera:", "screen:", "ndi:"] {
                if !id.starts_with(prefix) {
                    push(format!("{prefix}{id}"));
                }
            }
        }
        candidates
    }

    /// The media source id whose `video:` path ends with one of the prop's
    /// paths, matching the renderer's media-library-relative prop against the
    /// absolute path the engine was started with.
    pub fn matches_video_path(&self, source_id: &str) -> bool {
        let Some(source_path) = source_id.strip_prefix("video:") else {
            return false;
        };
        let Some(source_path) = normalise_library_path(source_path) else {
            return false;
        };
        self.paths.iter().any(|path| {
            normalise_library_path(path).is_some_and(|needle| source_path.ends_with(&needle))
        })
    }
}

fn kind_from_location(location: &str) -> String {
    match location {
        IMAGE_LOCATION => "image",
        VIDEO_LOCATION => "video",
        CANVAS_LOCATION => "canvas",
        _ => "",
    }
    .to_string()
}

/// Extensions the `image` crate decodes (its default feature set), used to
/// decide whether a non-`image` texture prop's `options.path` is worth a decode
/// attempt.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tga", "qoi", "tif", "tiff", "hdr", "exr", "ico",
    "pnm", "ppm", "pgm", "dds",
];

fn has_image_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_ascii_lowercase();
            IMAGE_EXTENSIONS.contains(&extension.as_str())
        })
}

/// One decoded image, tightly packed RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// `width * height * 4` bytes, row-major.
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    fn from_dynamic(image: image::DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        Self {
            width,
            height,
            rgba: rgba.into_raw(),
        }
    }

    /// Downscale to fit `max_size` (aspect preserved). Never upscales.
    fn fit_within(self, max_size: (u32, u32)) -> Self {
        let (max_width, max_height) = (max_size.0.max(1), max_size.1.max(1));
        if self.width <= max_width && self.height <= max_height {
            return self;
        }
        // Fit inside the box without changing the aspect ratio.
        let scale = (f64::from(max_width) / f64::from(self.width))
            .min(f64::from(max_height) / f64::from(self.height));
        let target_width = ((f64::from(self.width) * scale).round() as u32).max(1);
        let target_height = ((f64::from(self.height) * scale).round() as u32).max(1);
        let image = image::RgbaImage::from_raw(self.width, self.height, self.rgba)
            .expect("the decoded buffer matches its dimensions");
        let resized = image::imageops::resize(
            &image,
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        );
        Self {
            width: resized.width(),
            height: resized.height(),
            rgba: resized.into_raw(),
        }
    }

    /// `width * height * 4`, the tightly packed upload size.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }
}

/// Decode image bytes (PNG, JPEG, and the other enabled `image` codecs) into
/// tightly packed RGBA8.
#[cfg_attr(not(test), allow(dead_code))]
pub fn decode_rgba8(bytes: &[u8]) -> Result<DecodedImage, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("could not decode the image: {error}"))?;
    Ok(DecodedImage::from_dynamic(image))
}

/// Decode an image file into tightly packed RGBA8.
///
/// The file is refused when it is larger than [`MAX_IMAGE_PIXELS`] *after*
/// decoding (the dimensions are only known from the decoded image; the file
/// size guard keeps a pathological decode from starting).
pub fn decode_file(path: &Path) -> Result<DecodedImage, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("could not stat the image: {error}"))?;
    if !metadata.is_file() {
        return Err("the image path is not a file".to_string());
    }
    let image =
        image::open(path).map_err(|error| format!("could not decode the image: {error}"))?;
    let decoded = DecodedImage::from_dynamic(image);
    if u64::from(decoded.width) * u64::from(decoded.height) > MAX_IMAGE_PIXELS {
        return Err(format!(
            "the image is {}x{} (more than {MAX_IMAGE_PIXELS} pixels)",
            decoded.width, decoded.height
        ));
    }
    Ok(decoded)
}

/// A decoded image uploaded as a sampled GPU texture.
#[derive(Clone)]
pub struct ResolvedImage {
    /// Sampled view of the uploaded texture.
    pub view: wgpu::TextureView,
    /// Uploaded size (after any requested downscale). Part of the resolved
    /// contract for texture sources that need intrinsic dimensions
    /// (`Texture 2D`'s crop/constrain maths).
    #[allow(dead_code)]
    pub size: (u32, u32),
    /// Absolute path the image was resolved from.
    #[allow(dead_code)]
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    path: PathBuf,
    max_size: Option<(u32, u32)>,
}

struct CachedImage {
    /// Backing texture; the view below keeps it alive but this documents
    /// ownership and lets the cache measure/report sizes.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    last_used: u64,
}

/// Bounded decoded-image texture cache.
///
/// Keyed by `(absolute path, requested size)`: a Gallery thumbnail asks for a
/// small decode while a live ISF input uses the native size, and both keep
/// their own GPU texture. Paths are re-stat'ed on every miss, so replacing an
/// image file at the same path is picked up after the entries are evicted
/// (`set_media_root` and [`ImageTextures::clear`] clear immediately).
pub struct ImageTextures {
    cache: HashMap<ImageKey, CachedImage>,
    media_root: Option<PathBuf>,
    tick: u64,
    max_entries: usize,
}

impl Default for ImageTextures {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageTextures {
    /// Empty cache with the default bound.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            media_root: None,
            tick: 0,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    /// Point the resolver at the project media directory and drop every cached
    /// texture (paths may resolve differently under the new root).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_media_root(&mut self, root: impl Into<PathBuf>) {
        let root = root.into();
        if self.media_root.as_ref() != Some(&root) {
            self.media_root = Some(root);
            self.cache.clear();
        }
    }

    /// The configured media root, without the environment/default fallbacks.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn configured_root(&self) -> Option<&Path> {
        self.media_root.as_deref()
    }

    /// The root relative image paths resolve against.
    pub fn media_root(&self) -> Option<PathBuf> {
        self.media_root
            .clone()
            .or_else(|| {
                std::env::var_os("MODV_MEDIA_DIR")
                    .map(PathBuf::from)
                    .filter(|root| !root.as_os_str().is_empty())
            })
            .or_else(default_media_dir)
    }

    /// Drop every cached texture.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Number of cached textures (diagnostic/test use).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty (diagnostic/test use).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Resolve a saved texture path to an absolute file path.
    ///
    /// An existing absolute path wins (an external image); otherwise the path
    /// is treated as media-library relative and joined under the media root.
    pub fn resolve_path(&self, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        let raw = trimmed.strip_prefix("file://").unwrap_or(trimmed);
        let normalised = raw.replace('\\', "/");
        if normalised.contains('\0') || normalised.split('/').any(|segment| segment == "..") {
            return None;
        }

        let direct = Path::new(&normalised);
        if direct.is_absolute() && direct.is_file() {
            return Some(direct.to_path_buf());
        }

        let relative = normalise_library_path(&normalised)?;
        let root = self.media_root()?;
        let joined = root.join(relative);
        joined.is_file().then_some(joined)
    }

    /// Upload (or reuse) the image at `raw`, downscaling to `max_size` when
    /// given.
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raw: &str,
        max_size: Option<(u32, u32)>,
    ) -> Result<ResolvedImage, String> {
        let path = self
            .resolve_path(raw)
            .ok_or_else(|| format!("no image file could be resolved from {raw:?}"))?;
        let key = ImageKey {
            path: path.clone(),
            max_size,
        };

        self.tick += 1;
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.last_used = self.tick;
            return Ok(ResolvedImage {
                view: entry.view.clone(),
                size: entry.size,
                path,
            });
        }

        let mut decoded = decode_file(&path)?;
        if let Some(max_size) = max_size {
            decoded = decoded.fit_within(max_size);
        }
        let (width, height) = (decoded.width.max(1), decoded.height.max(1));

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("modv-image-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &decoded.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Evict the least recently used entry before inserting; the cache is a
        // handful of full-resolution textures, not a streaming pool.
        while self.cache.len() >= self.max_entries {
            let Some(oldest) = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.cache.remove(&oldest);
        }

        self.cache.insert(
            key,
            CachedImage {
                _texture: texture,
                view: view.clone(),
                size: (width, height),
                last_used: self.tick,
            },
        );

        Ok(ResolvedImage {
            view,
            size: (width, height),
            path,
        })
    }
}

/// Electron's userData `media` directory for `app.setName("VmodV")`.
///
/// Linux follows `XDG_CONFIG_HOME` (Electron's `app.getPath("userData")`) and
/// falls back to `~/.config`. macOS and Windows use their Electron defaults.
fn default_media_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/VmodV/media"))
    }
    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(app_data).join("VmodV/media"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()) {
            return Some(PathBuf::from(config).join("VmodV/media"));
        }
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".config/VmodV/media"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unique_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modv-texture-sources-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_png(path: &Path, width: u32, height: u32, colour: [u8; 4]) {
        let mut image = image::RgbaImage::new(width, height);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba(colour);
        }
        image.save(path).expect("png written");
    }

    #[test]
    fn parses_the_saved_texture_definition() {
        let request = TextureRequest::from_value(Some(&json!({
            "type": "image",
            "options": { "path": "default/image/photo.png" }
        })))
        .expect("a typed definition parses");
        assert_eq!(request.kind, "image");
        assert!(request.wants_image());
        assert_eq!(request.first_image_path(), Some("default/image/photo.png"));

        let legacy = TextureRequest::from_value(Some(&json!({
            "location": "images/image",
            "id": "runtime-image-id",
            "options": { "path": "/media/image/photo.png" }
        })))
        .expect("a legacy definition parses");
        assert_eq!(legacy.kind, "image");
        assert_eq!(legacy.ids, vec!["runtime-image-id"]);
        assert_eq!(legacy.paths, vec!["/media/image/photo.png"]);
    }

    #[test]
    fn canvas_and_video_definitions_name_ids_and_source_prefixes() {
        let canvas = TextureRequest::from_value(Some(&json!({
            "type": "canvas",
            "options": { "id": "module-a" }
        })))
        .expect("canvas definition");
        assert_eq!(canvas.ids, vec!["module-a"]);
        assert!(canvas.media_ids().contains(&"camera:module-a".to_string()));

        let prefixed = TextureRequest::from_value(Some(&json!({
            "type": "webcam",
            "options": { "id": "camera:cam-a" }
        })))
        .expect("prefixed definition");
        assert!(
            prefixed.media_ids().contains(&"cam-a".to_string()),
            "the bare source key is tried for a prefixed id"
        );

        let video = TextureRequest::from_value(Some(&json!({
            "type": "video",
            "options": { "path": "default/video/clip.mp4" }
        })))
        .expect("video definition");
        assert_eq!(video.kind, "video");
        assert!(video.matches_video_path("video:/home/u/VmodV/media/default/video/clip.mp4"));
        assert!(!video.matches_video_path("camera:/dev/video0"));
        assert!(!video.matches_video_path("video:/home/u/other/clip.mp4"));
    }

    #[test]
    fn unassigned_and_primitive_values_are_handled() {
        assert_eq!(TextureRequest::from_value(None), None);
        assert_eq!(TextureRequest::from_value(Some(&Value::Null)), None);
        assert_eq!(
            TextureRequest::from_value(Some(&json!(""))),
            None,
            "an empty string is the JS canvas default"
        );
        assert_eq!(
            TextureRequest::from_value(Some(&json!(7))),
            None,
            "a number is not a texture definition"
        );
        let bare = TextureRequest::from_value(Some(&json!("module-a"))).expect("bare string");
        assert_eq!(bare.ids, vec!["module-a"]);
        assert_eq!(bare.paths, vec!["module-a"]);
    }

    #[test]
    fn only_image_paths_reach_the_decoder() {
        let video = TextureRequest::from_value(Some(&json!({
            "type": "video",
            "options": { "path": "default/video/clip.mp4" }
        })))
        .expect("video definition");
        assert!(
            video.image_paths().is_empty(),
            "a video path must not be fed to the image decoder"
        );
        assert_eq!(video.first_image_path(), None);

        let image = TextureRequest::from_value(Some(&json!({
            "type": "image",
            "options": { "path": "default/image/photo.PNG" }
        })))
        .expect("image definition");
        assert_eq!(image.image_paths(), vec!["default/image/photo.PNG"]);

        let canvas = TextureRequest::from_value(Some(&json!({
            "type": "canvas",
            "options": { "path": "default/image/photo.jpg" }
        })))
        .expect("canvas definition");
        assert_eq!(
            canvas.first_image_path(),
            Some("default/image/photo.jpg"),
            "a non-image prop still binds an image path when the extension says so"
        );
    }

    #[test]
    fn group_and_inherit_requests_have_no_sources() {
        for value in [json!({"type": "group"}), json!({"type": "inherit"})] {
            let request = TextureRequest::from_value(Some(&value)).expect("parses");
            assert!(request.ids.is_empty());
            assert!(request.paths.is_empty());
            assert!(!request.wants_image());
        }
    }

    #[test]
    fn decodes_png_bytes_to_tightly_packed_rgba8() {
        let dir = unique_dir("decode-bytes");
        let path = dir.join("probe.png");
        write_png(&path, 3, 2, [10, 20, 30, 255]);
        let bytes = std::fs::read(&path).expect("png read");

        let decoded = decode_rgba8(&bytes).expect("png decodes");
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(decoded.byte_len(), 3 * 2 * 4);
        assert_eq!(&decoded.rgba[0..4], &[10, 20, 30, 255]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolves_media_library_paths_under_the_root() {
        let dir = unique_dir("resolve-root");
        let nested = dir.join("default/image");
        std::fs::create_dir_all(&nested).expect("media folders");
        write_png(&nested.join("photo.png"), 1, 1, [255, 255, 255, 255]);

        let mut images = ImageTextures::new();
        images.set_media_root(&dir);

        let resolved = images
            .resolve_path("default/image/photo.png")
            .expect("library path resolves");
        assert_eq!(resolved, nested.join("photo.png"));
        assert_eq!(
            images.resolve_path("/default/image/photo.png"),
            Some(nested.join("photo.png")),
            "a leading separator is a media-library path, not an absolute path"
        );
        assert_eq!(
            images.resolve_path(&nested.join("photo.png").to_string_lossy()),
            Some(nested.join("photo.png")),
            "an existing absolute path is used as-is"
        );
        assert_eq!(images.resolve_path("default/image/missing.png"), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_parent_traversal_and_unresolvable_paths() {
        let dir = unique_dir("traversal");
        let mut images = ImageTextures::new();
        images.set_media_root(&dir);

        for path in [
            "../secret.png",
            "default/../../secret.png",
            "/default/../secret.png",
            "default/secret\0.png",
            "",
            "   ",
        ] {
            assert_eq!(images.resolve_path(path), None, "{path:?} must be rejected");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn media_root_changes_clear_the_cache() {
        let dir = unique_dir("cache-clear");
        let mut images = ImageTextures::new();
        images.set_media_root(dir.join("a"));
        assert_eq!(images.configured_root(), Some(dir.join("a").as_path()));
        images.set_media_root(dir.join("b"));
        assert_eq!(images.configured_root(), Some(dir.join("b").as_path()));
        images.clear();
        assert!(images.is_empty());
        assert_eq!(images.len(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
