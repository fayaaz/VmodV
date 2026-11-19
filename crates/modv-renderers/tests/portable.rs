//! RED-phase tests for the portable-Windows asset-root contract (test only).
//!
//! These live in an integration test rather than the in-file `#[cfg(test)]`
//! module of `point10::shared::assets` because the whole `modv-renderers`
//! crate is `#![forbid(unsafe_code)]` (inherited by in-crate test modules),
//! while Rust 2024 requires `unsafe { std::env::set_var(..) }` to mutate the
//! process environment. Integration tests are separate crates, so the forbid
//! does not apply. The production API used here is entirely public.

use std::path::{Path, PathBuf};

use modv_renderers::point10::shared::assets::{ASSETS_DIR_ENV, AssetLoader};

const PORTABLE_ROOT_ENV: &str = "MODV_PORTABLE_ROOT";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vmodv-portable-{}-{nanos}-{tag}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp portable root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: these tests run serially (--test-threads=1) and no other
        // thread in this test process reads the modified variables while the
        // guard is alive.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see EnvGuard::set; the guard restores the exact value it
        // observed before mutation.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
fn asset_loader_includes_portable_root_textures_after_assets_dir_env() {
    let root = TempDir::new("textures");
    let textures = root.path().join("textures");
    std::fs::create_dir_all(&textures).expect("create portable textures dir");

    let assets_dir = root.path().join("assets-from-env");
    std::fs::create_dir_all(&assets_dir).expect("create assets-env dir");

    // Clear the competing env var first, then set both under test. Each guard
    // restores its prior value on drop, so the suite is serial-safe.
    let _assets_env = EnvGuard::set(ASSETS_DIR_ENV, &assets_dir);
    let _portable_env = EnvGuard::set(PORTABLE_ROOT_ENV, root.path());

    let loader = AssetLoader::for_assets_dir(None);
    let dirs = loader.dirs();

    assert!(
        dirs.contains(&textures),
        "expected <MODV_PORTABLE_ROOT>/textures ({}) among the asset candidates {:?}",
        textures.display(),
        dirs
    );

    let assets_pos = dirs
        .iter()
        .position(|dir| dir == &assets_dir)
        .expect("$MODV_ASSETS_DIR candidate must be retained");
    let portable_pos = dirs
        .iter()
        .position(|dir| dir == &textures)
        .expect("portable-root candidate must be present");
    assert!(
        assets_pos < portable_pos,
        "$MODV_ASSETS_DIR (position {assets_pos}) must take priority over \
         the portable-root candidate (position {portable_pos}); dirs = {dirs:?}"
    );
}
