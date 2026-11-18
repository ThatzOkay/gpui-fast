The GLES 2 backend lives in this checkout. The companion app at
`../gpui-test` fetches this fork and the companion wgpu fork through Cargo.

The GLES 2 renderer lives in `crates/gpui_wgpu/src/fast/gles2/`:

- `geometry.rs` expands scene rectangles and glyphs into float vertices.
- `shader.wgsl` translates to GLSL ES 1.00 without optional extensions.
- `shapes.rs` packs rounded geometry into six float uniform vectors.
- `scene.rs` preserves batch order, plans offscreen passes, and caches retained tiles.
- `mod.rs` owns pipelines, texture bindings, vertex uploads, and surface submission.

Adapters reporting shader model `Sm2` select this path before creating GPUI's
GLES 3 pipelines. Opaque solids, alpha solids, glyphs, color images, and rounded
fills and borders use separate fragment entry points. Dashed borders apply a
rectangle-perimeter mask to a separate RGBA8 border target. Rounded geometry uses ordinary float
uniforms; retained layers and rounded overflow groups use RGBA8 render targets.
No float textures, integer attributes, storage buffers, or instancing are required.
Retained tiles reuse their pixels until dirty, and repaint after missed generations.
Additional primitive batches belong in `scene.rs`. Unsupported primitive kinds
emit a warning once per renderer.

The redirected `fast/wgpu_atlas.rs` expands monochrome coverage bytes into RGBA8
textures for GLES 2. The red channel retains glyph coverage. The redirected
`fast/wgpu_renderer.rs` connects selection, drawing, resize, surface replacement,
and resource release to the GLES 2 implementation. Original upstream files remain
alongside these redirects for comparison and future syncing.

Validation from this workspace:

```sh
cargo test -p gpui_wgpu --lib gles2
cargo test -p gpui_linux --no-default-features --features wayland,gpui/font-kit --lib touch
```

The tests check geometry, translate every shader entry point with the pinned
GLES 2 Naga backend, and check touch contact lifetimes. Build the tablet binary
with `../gpui-test/build-surface-rt.sh`.

The vertex shader takes individual float attributes and constructs its output
in one expression.

`../wgpu/wgpu-hal` owns the GLES API integration. On GLES 2, only point-list pipelines emit the
automatic `gl_PointSize` output. Emitting it for triangles corrupted varyings
on the tablet. The shader program cache includes this choice so point and
triangle pipelines cannot share an incompatible program.

`crates/gpui_linux` owns the Wayland input integration. Its client is
redirected into `src/fast/wayland_client.rs`; `fast/wayland_touch/` binds `wl_touch`
and forwards frame-batched contacts to GPUI's existing raw-touch API. Protocol
contact IDs may be reused, so this bridge allocates fresh `TouchId` values and
keeps their target window fixed for each contact. Cancellation, seat replacement,
and window closure clean up active contacts.

## Opt-in frame timing

Build both local forks into the application, then run:

```sh
GPUI_FRAME_TIMING=1 WGPU_FRAME_TIMING=1 \
RUST_LOG=info ./gpui-test
```

`gpui_frame_timing` reports `ui_draw` (total CPU scene construction, layout, and paint),
`window_present`, and `gles2_render` (surface acquisition, scene planning,
bindings/shapes, vertex upload, encoding, submission, presentation).
`wgpu_frame_timing` reports `gl_submit` (context lock, command replay,
fallback GLES 2 `glFinish` wait, fence/flush) and `egl_present` (context setup,
resolve/blit, swap, context release in tail).

Each line includes a scope-local sample ID, a Unix start timestamp in microseconds,
total elapsed milliseconds, and successive non-overlapping stage durations.
Nested scopes overlap: do not add renderer submit time to HAL submit time.
These are CPU wall-clock measurements, not GPU timestamp queries. In particular,
`finish_wait` includes waiting for previously queued GPU work, and EGL swap may
include compositor/vsync backpressure. No extra GPU synchronization is inserted.
Timers and allocations stay disabled unless their environment flag is `1` or `true`.
Logging itself can affect frame pacing; capture short runs and compare with logging off.

This app pins Git revisions. Local fork edits require Cargo path patches (including
wgpu) or newly published revisions before they appear in a tablet build.

## Surface RT performance

The GLES 2 renderer trims rounded fills and borders to avoid unnecessary
fragment work, skips transparent fills, and folds a leading opaque full-window
background into the render-pass clear when its coverage is exact. Primitive
order, content masks, translucent colors, and rounded edges remain respected.
Set `GPUI_GLES2_SHAPE_TRIMMING=0` or `GPUI_GLES2_CLEAR_BACKGROUND=0` to disable
the respective optimization for comparisons.

When the platform requests compositor-paced presentation, the renderer uses
Immediate mode only if the EGL configuration supports a zero swap interval;
otherwise it retains FIFO. The pinned companion wgpu revision enables
GL_APPLE_sync fences only when the extension and all required functions are
available. Unsupported contexts retain the synchronous GLES 2 fallback.
The backend also caches texture sampler parameters within a command buffer
and avoids a temporary CPU allocation when copying between distinct buffers.

On the tablet, controlled full-window background comparisons averaged about
50 FPS with folding enabled versus 39–41 FPS without it. These measurements
include scene and compositor work and vary with power settings and workload.
The rejected upload-reuse experiment is not included. App-specific overlay
changes are maintained in the demo application, not in this renderer.
