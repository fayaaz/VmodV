//! The compiled-in Point10 texture bundle.
//!
//! The wasm browser shell has no filesystem, so `Path::is_file()` is always
//! false there and [`AssetLoader`](super::assets::AssetLoader) cannot resolve a
//! texture from disk. This module embeds every shipped
//! `resources/point10/textures/**/*.jpg|*.png` file with `include_bytes!` so
//! [`AssetLoader::load`](super::assets::AssetLoader::load) can decode the same
//! bytes in memory as a last resort.
//!
//! Only the decodable raster formats are bundled: `dvd_logo.svg` is a vector
//! asset that the `image` crate does not decode, so it is excluded.
//!
//! The module is gated to `cfg(any(test, target_arch = "wasm32"))` at its
//! declaration so desktop release binaries do not carry the ~6.2 MB bundle;
//! `cfg(test)` keeps it unit-testable on the native host.

#![forbid(unsafe_code)]

/// Look up a texture by its `/`-separated path relative to
/// `resources/point10/textures` (e.g. `earth.jpg`, `stormy/posx.jpg`).
///
/// Returns the raw file bytes, or `None` for names that are not bundled.
pub fn lookup(name: &str) -> Option<&'static [u8]> {
    let bytes: &'static [u8] = match name {
        "checkerboard_diffuse.png" => {
            include_bytes!("../../../../../resources/point10/textures/checkerboard_diffuse.png")
        }
        "concrete044c_col_1k.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/concrete044c_col_1k.jpg")
        }
        "earth.jpg" => include_bytes!("../../../../../resources/point10/textures/earth.jpg"),
        "earthcloud.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/earthcloud.jpg")
        }
        "earthlights.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/earthlights.jpg")
        }
        "moon_04_diff_1k.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/moon_04_diff_1k.jpg")
        }
        "rubber002_diff_1k.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/rubber002_diff_1k.jpg")
        }
        "rubber002_rough_1k.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/rubber002_rough_1k.jpg")
        }
        "spruit_sunrise_2k.hdr.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/spruit_sunrise_2k.hdr.jpg")
        }
        "steel_diffuse.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/steel_diffuse.jpg")
        }
        "tinore_ao.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/tinore_ao.jpg")
        }
        "tinore_diffuse.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/tinore_diffuse.jpg")
        }
        "tinore_metallic.png" => {
            include_bytes!("../../../../../resources/point10/textures/tinore_metallic.png")
        }
        "tinore_normal.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/tinore_normal.jpg")
        }
        "tinore_roughness.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/tinore_roughness.jpg")
        }
        "waternormals.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/waternormals.jpg")
        }
        "wilkyway_panorama.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/wilkyway_panorama.jpg")
        }
        "stormy/negx.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/negx.jpg")
        }
        "stormy/negy.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/negy.jpg")
        }
        "stormy/negz.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/negz.jpg")
        }
        "stormy/posx.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/posx.jpg")
        }
        "stormy/posy.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/posy.jpg")
        }
        "stormy/posz.jpg" => {
            include_bytes!("../../../../../resources/point10/textures/stormy/posz.jpg")
        }
        _ => return None,
    };
    Some(bytes)
}
