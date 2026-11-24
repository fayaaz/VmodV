//! Surface creation and presentation target for a window.

use std::sync::Arc;

use wgpu::CurrentSurfaceTexture;
use winit::window::Window;

use crate::device::{Engine, EngineError};

/// A configured wgpu surface bound to a winit window.
pub struct SurfaceTarget {
    /// The surface presenting into the window.
    pub surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// Create and configure a surface for `window`.
///
/// The adapter must be able to present to the surface; the first supported
/// sRGB format (otherwise the first supported format) is selected. Fifo
/// (vsync) presentation is the default. The initial configuration size is
/// taken from
/// [`Window::inner_size`], clamped to a non-zero size.
pub fn create_surface(engine: &Engine, window: Arc<Window>) -> Result<SurfaceTarget, EngineError> {
    create_surface_with_present_mode(engine, window, wgpu::PresentMode::Fifo)
}

/// Pick the present mode to configure a surface with.
///
/// [`wgpu::PresentMode::AutoVsync`] and [`wgpu::PresentMode::AutoNoVsync`] are
/// meta-modes: wgpu resolves them to a supported concrete mode itself, and they
/// never appear in [`wgpu::SurfaceCapabilities::present_modes`]. They must be
/// passed through unchanged — demoting a secondary Output Window to Fifo can
/// stall the shared frame loop when that window is occluded (GNOME/Wayland
/// withholds frame callbacks), dragging down the visible UI's FPS. Concrete
/// modes are kept when supported; otherwise Fifo is used.
fn resolve_present_mode(
    requested: wgpu::PresentMode,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    match requested {
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync => requested,
        mode if supported.contains(&mode) => mode,
        _ => wgpu::PresentMode::Fifo,
    }
}

/// [`create_surface`] with an explicit present mode.
///
/// Secondary Output Windows pass [`wgpu::PresentMode::AutoNoVsync`] so they
/// cannot throttle the primary window's Fifo-paced frame loop. The requested
/// mode is checked against the surface's capabilities by
/// `resolve_present_mode`.
pub fn create_surface_with_present_mode(
    engine: &Engine,
    window: Arc<Window>,
    present_mode: wgpu::PresentMode,
) -> Result<SurfaceTarget, EngineError> {
    let size = window.inner_size();
    let width = size.width.max(1);
    let height = size.height.max(1);
    let surface = engine
        .instance()
        .create_surface(window)
        .map_err(|e| EngineError::CreateSurface(e.to_string()))?;
    let caps = surface.get_capabilities(engine.adapter());
    if caps.formats.is_empty() {
        return Err(EngineError::NoSurfaceFormats);
    }
    let format = caps
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .unwrap_or(caps.formats[0]);
    let present_mode = resolve_present_mode(present_mode, &caps.present_modes);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        color_space: wgpu::SurfaceColorSpace::Auto,
        width,
        height,
        present_mode,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&engine.device, &config);
    Ok(SurfaceTarget { surface, config })
}

impl SurfaceTarget {
    /// Reconfigure the surface to a new size. Zero-sized dimensions are ignored.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(device, &self.config);
    }

    /// The dimensions the surface is currently configured for.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The texture format the surface is configured for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Acquire the next surface texture to render into.
    ///
    /// wgpu 30 returns [`CurrentSurfaceTexture`] rather than a `Result`:
    /// callers must handle `Timeout`/`Occluded` (skip the frame) and
    /// `Outdated`/`Lost` (reconfigure or recreate the surface) themselves.
    pub fn current_texture(&self) -> CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }

    /// Reconfigure the surface with its current configuration, e.g. after an
    /// `Outdated`/`Lost`/`Suboptimal` frame acquisition.
    pub fn reconfigure(&mut self, device: &wgpu::Device) {
        self.surface.configure(device, &self.config);
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_present_mode;
    use wgpu::PresentMode;

    #[test]
    fn auto_no_vsync_present_mode_is_passed_through_to_wgpu() {
        let resolved = resolve_present_mode(
            PresentMode::AutoNoVsync,
            &[
                PresentMode::Fifo,
                PresentMode::Mailbox,
                PresentMode::Immediate,
            ],
        );

        assert_eq!(resolved, PresentMode::AutoNoVsync);
    }

    #[test]
    fn auto_vsync_present_mode_is_passed_through_to_wgpu() {
        let resolved = resolve_present_mode(PresentMode::AutoVsync, &[PresentMode::Fifo]);

        assert_eq!(resolved, PresentMode::AutoVsync);
    }

    #[test]
    fn a_supported_concrete_present_mode_is_kept() {
        let resolved = resolve_present_mode(
            PresentMode::Mailbox,
            &[PresentMode::Fifo, PresentMode::Mailbox],
        );

        assert_eq!(resolved, PresentMode::Mailbox);
    }

    #[test]
    fn an_unsupported_concrete_present_mode_falls_back_to_fifo() {
        let resolved = resolve_present_mode(PresentMode::Immediate, &[PresentMode::Fifo]);

        assert_eq!(resolved, PresentMode::Fifo);
    }
}
