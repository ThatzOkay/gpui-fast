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
