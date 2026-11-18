mod cosmic_text_system;
mod fast;
#[path = "fast/wgpu_atlas.rs"]
mod wgpu_atlas;
mod wgpu_context;
#[path = "fast/wgpu_renderer.rs"]
mod wgpu_renderer;

pub use cosmic_text_system::*;
pub use fast::dynamic_textures::WgpuDynamicTexture;
#[cfg(not(target_family = "wasm"))]
pub use fast::wgpu_context::wgpu_context;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub use wgpu_renderer::WgpuHeadlessRenderer;
pub use wgpu_renderer::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};
