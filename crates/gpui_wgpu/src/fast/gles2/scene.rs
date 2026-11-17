use super::{
    Draw, PipelineKind,
    geometry::{Rectangle, Vertex, scissor},
    shapes::{Shape, radii},
};
use crate::WgpuAtlas;
use gpui::{
    AtlasTextureId, BorderStyle, Bounds, Edges, LayerContent, LayerFrame, Point, PolychromeSprite,
    PrimitiveBatch, Rgba, ScaledPixels, Scene, TransformationMatrix, decode_layer_tile, point,
    size, transparent_black, white,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    Tile { layer: u32, x: i32, y: i32 },
    Group(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Texture {
    Atlas(AtlasTextureId),
    Target(Target),
}

struct CachedTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    generation: u64,
    content: Option<LayerContent>,
    background: Rgba,
    used: u64,
}
#[derive(Default)]
pub(super) struct Targets {
    items: HashMap<Target, CachedTarget>,
    epoch: u64,
}
impl Targets {
    pub fn view(&self, key: Target) -> &wgpu::TextureView {
        &self.items[&key].view
    }
    fn ensure(&mut self, device: &wgpu::Device, key: Target, size: [u32; 2]) -> bool {
        if let Some(target) = self.items.get_mut(&key)
            && target.size == size
        {
            target.used = self.epoch;
            return false;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gles2 retained target"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.items.insert(
            key,
            CachedTarget {
                view: texture.create_view(&Default::default()),
                _texture: texture,
                size,
                generation: 0,
                content: None,
                background: Rgba::default(),
                used: self.epoch,
            },
        );
        true
    }
}

pub(super) struct Pass {
    pub target: Option<Target>,
    pub clear: wgpu::Color,
    pub draws: Vec<Draw>,
}
#[derive(Default)]
pub(super) struct Plan {
    pub vertices: Vec<Vertex>,
    pub shapes: Vec<Shape>,
    pub passes: Vec<Pass>,
}

struct Planner<'a> {
    root: &'a Scene,
    device: &'a wgpu::Device,
    atlas: &'a WgpuAtlas,
    targets: &'a mut Targets,
    warned: &'a mut HashSet<&'static str>,
    active: HashSet<Target>,
    next_group: u32,
    plan: Plan,
}

pub(super) fn plan(
    scene: &Scene,
    viewport: [u32; 2],
    device: &wgpu::Device,
    atlas: &WgpuAtlas,
    targets: &mut Targets,
    warned: &mut HashSet<&'static str>,
) -> Plan {
    targets.epoch = targets.epoch.wrapping_add(1);
    let mut planner = Planner {
        root: scene,
        device,
        atlas,
        targets,
        warned,
        active: HashSet::new(),
        next_group: 0,
        plan: Plan::default(),
    };
    let draws = planner.scene(
        scene,
        viewport,
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        0,
    );
    planner.plan.passes.push(Pass {
        target: None,
        clear: wgpu::Color::BLACK,
        draws,
    });
    let epoch = planner.targets.epoch;
    planner
        .targets
        .items
        .retain(|_, target| epoch.wrapping_sub(target.used) <= 2);
    planner.plan
}

fn local_mask(bounds: Bounds<ScaledPixels>, origin: Point<ScaledPixels>) -> Bounds<ScaledPixels> {
    Bounds {
        origin: bounds.origin - origin,
        size: bounds.size,
    }
}
fn transform(origin: Point<ScaledPixels>) -> TransformationMatrix {
    TransformationMatrix {
        translation: [-origin.x.0, -origin.y.0],
        ..TransformationMatrix::unit()
    }
}
fn clear(color: Rgba) -> wgpu::Color {
    wgpu::Color {
        r: (color.r * color.a) as f64,
        g: (color.g * color.a) as f64,
        b: (color.b * color.a) as f64,
        a: color.a as f64,
    }
}

impl Planner<'_> {
    fn warn(&mut self, message: &'static str) {
        if self.warned.insert(message) {
            log::warn!("GLES 2 scene renderer does not yet support {message}");
        }
    }
    fn append(
        &mut self,
        draws: &mut Vec<Draw>,
        rectangle: Rectangle,
        mask: Bounds<ScaledPixels>,
        viewport: [u32; 2],
        texture: Option<Texture>,
        kind: PipelineKind,
        shape: Option<Shape>,
    ) {
        let Some(scissor) = scissor(mask, [viewport[0] as f32, viewport[1] as f32]) else {
            return;
        };
        let shape = shape.map(|shape| {
            self.plan.shapes.push(shape);
            self.plan.shapes.len() - 1
        });
        let start = self.plan.vertices.len() as u32;
        rectangle.append(
            [viewport[0] as f32, viewport[1] as f32],
            &mut self.plan.vertices,
        );
        let end = self.plan.vertices.len() as u32;
        if let Some(last) = draws.last_mut()
            && last.kind == kind
            && last.texture == texture
            && last.scissor == scissor
            && last.shape == shape
            && last.vertices.end == start
        {
            last.vertices.end = end;
        } else {
            draws.push(Draw {
                kind,
                scissor,
                shape,
                vertices: start..end,
                texture,
            });
        }
    }

    fn scene(
        &mut self,
        scene: &Scene,
        viewport: [u32; 2],
        origin: Point<ScaledPixels>,
        depth: usize,
    ) -> Vec<Draw> {
        if depth > 64 {
            self.warn("cyclic or excessively nested layer scenes");
            return Vec::new();
        }
        let mut draws = Vec::new();
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Quads(range) => {
                    for quad in &scene.quads[range] {
                        let Some(color) = quad.background.as_solid() else {
                            self.warn("gradient and pattern fills");
                            continue;
                        };
                        if color.a <= 0.0 && quad.border_color.a <= 0.0 {
                            continue;
                        }
                        let rounded = quad.corner_radii != Default::default()
                            || quad.border_widths != Default::default();
                        let kind = if rounded {
                            PipelineKind::Rounded
                        } else if color.a == 1.0 {
                            PipelineKind::SolidOpaque
                        } else {
                            PipelineKind::SolidAlpha
                        };
                        let uv = [0.0, 0.0, 1.0, 1.0];
                        let shape = rounded.then(|| {
                            Shape::new(
                                quad.bounds,
                                radii(quad.corner_radii),
                                radii(quad.corner_radii),
                                quad.border_widths,
                                quad.border_color,
                                quad.border_style,
                                uv,
                            )
                        });
                        self.append(
                            &mut draws,
                            Rectangle {
                                bounds: quad.bounds,
                                uv,
                                color,
                                transform: transform(origin),
                            },
                            local_mask(quad.content_mask.bounds, origin),
                            viewport,
                            None,
                            kind,
                            shape,
                        );
                        if quad.border_color.a > 0.0 && quad.border_widths != Default::default() {
                            self.border(&mut draws, quad, shape.unwrap(), viewport, origin);
                        }
                    }
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    let Some(size) = self.atlas.texture_size(texture_id) else {
                        continue;
                    };
                    for sprite in &scene.monochrome_sprites[range] {
                        let tile = sprite.tile.bounds;
                        let mut transformation = sprite.transformation;
                        transformation.translation[0] -= origin.x.0;
                        transformation.translation[1] -= origin.y.0;
                        self.append(
                            &mut draws,
                            Rectangle {
                                bounds: sprite.bounds,
                                uv: [
                                    tile.origin.x.0 as f32 / size[0],
                                    tile.origin.y.0 as f32 / size[1],
                                    tile.size.width.0 as f32 / size[0],
                                    tile.size.height.0 as f32 / size[1],
                                ],
                                color: sprite.color,
                                transform: transformation,
                            },
                            local_mask(sprite.content_mask.bounds, origin),
                            viewport,
                            Some(Texture::Atlas(texture_id)),
                            PipelineKind::Monochrome,
                            None,
                        );
                    }
                }
                PrimitiveBatch::PolychromeSprites { range, .. } => {
                    for sprite in &scene.polychrome_sprites[range] {
                        if let Some(group) =
                            crate::fast::rounded_clip::group(scene, sprite.tile.texture_id)
                        {
                            self.group(&mut draws, group, sprite, viewport, origin, depth + 1);
                        } else if let Some((key, tile)) =
                            decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id)
                        {
                            let layer = scene
                                .layers
                                .frames
                                .iter()
                                .chain(self.root.layers.frames.iter())
                                .find(|frame| frame.key == key)
                                .cloned();
                            if let Some(layer) = layer {
                                if let Some(target) = self.tile(&layer, tile, depth + 1) {
                                    self.sprite(
                                        &mut draws,
                                        sprite,
                                        viewport,
                                        origin,
                                        Texture::Target(target),
                                        [layer.tile_size as f32; 2],
                                        true,
                                    );
                                }
                            } else {
                                self.warn("missing retained-layer content");
                            }
                        } else if let Some(size) = self.atlas.texture_size(sprite.tile.texture_id) {
                            self.sprite(
                                &mut draws,
                                sprite,
                                viewport,
                                origin,
                                Texture::Atlas(sprite.tile.texture_id),
                                size,
                                false,
                            );
                        }
                    }
                }
                PrimitiveBatch::Shadows(_) => self.warn("shadows"),
                PrimitiveBatch::Paths(_) => self.warn("paths"),
                PrimitiveBatch::Underlines(_) => self.warn("underlines"),
                PrimitiveBatch::SubpixelSprites { .. } => self.warn("subpixel sprites"),
                PrimitiveBatch::Surfaces(_) => self.warn("native surfaces"),
            }
        }
        draws
    }

    fn border(
        &mut self,
        draws: &mut Vec<Draw>,
        quad: &gpui::Quad,
        mut shape: Shape,
        viewport: [u32; 2],
        origin: Point<ScaledPixels>,
    ) {
        let rectangle = Rectangle {
            bounds: quad.bounds,
            uv: [0.0, 0.0, 1.0, 1.0],
            color: quad.border_color,
            transform: transform(origin),
        };
        let mask = local_mask(quad.content_mask.bounds, origin);
        if quad.border_style != BorderStyle::Dashed {
            self.append(
                draws,
                rectangle,
                mask,
                viewport,
                None,
                PipelineKind::Border,
                Some(shape),
            );
            return;
        }
        let bounds = local_mask(quad.bounds, origin);
        let left = bounds.origin.x.0.max(mask.origin.x.0).max(0.0).floor();
        let top = bounds.origin.y.0.max(mask.origin.y.0).max(0.0).floor();
        let right = bounds
            .right()
            .0
            .min(mask.right().0)
            .min(viewport[0] as f32)
            .ceil();
        let bottom = bounds
            .bottom()
            .0
            .min(mask.bottom().0)
            .min(viewport[1] as f32)
            .ceil();
        if right <= left || bottom <= top {
            return;
        }
        let extent = [(right - left) as u32, (bottom - top) as u32];
        let target = Target::Group(self.next_group);
        self.next_group += 1;
        self.targets.ensure(self.device, target, extent);
        let child_origin = origin + point(ScaledPixels(left), ScaledPixels(top));
        let mut border = Vec::new();
        self.append(
            &mut border,
            Rectangle {
                transform: transform(child_origin),
                ..rectangle
            },
            local_mask(quad.content_mask.bounds, child_origin),
            extent,
            None,
            PipelineKind::Border,
            Some(shape),
        );
        self.plan.passes.push(Pass {
            target: Some(target),
            clear: wgpu::Color::TRANSPARENT,
            draws: border,
        });
        shape.mapping = [
            extent[0] as f32,
            extent[1] as f32,
            child_origin.x.0 - quad.bounds.origin.x.0,
            child_origin.y.0 - quad.bounds.origin.y.0,
        ];
        self.append(
            draws,
            Rectangle {
                bounds: Bounds {
                    origin: child_origin,
                    size: size(
                        ScaledPixels(extent[0] as f32),
                        ScaledPixels(extent[1] as f32),
                    ),
                },
                uv: [0.0, 0.0, 1.0, 1.0],
                color: white(),
                transform: transform(origin),
            },
            mask,
            viewport,
            Some(Texture::Target(target)),
            PipelineKind::BorderDashed,
            Some(shape),
        );
    }

    fn sprite(
        &mut self,
        draws: &mut Vec<Draw>,
        sprite: &PolychromeSprite,
        viewport: [u32; 2],
        origin: Point<ScaledPixels>,
        texture: Texture,
        size: [f32; 2],
        premult: bool,
    ) {
        let tile = sprite.tile.bounds;
        let uv = [
            tile.origin.x.0 as f32 / size[0],
            tile.origin.y.0 as f32 / size[1],
            tile.size.width.0 as f32 / size[0],
            tile.size.height.0 as f32 / size[1],
        ];
        let rounded = sprite.corner_radii != Default::default();
        let grey: bool = sprite.grayscale == true.into();
        let kind = match (premult, rounded, grey) {
            (false, false, false) => PipelineKind::Color,
            (false, false, true) => PipelineKind::ColorGray,
            (false, true, false) => PipelineKind::ColorRounded,
            (false, true, true) => PipelineKind::ColorRoundedGray,
            (true, false, false) => PipelineKind::Premult,
            (true, false, true) => PipelineKind::PremultGray,
            (true, true, false) => PipelineKind::PremultRounded,
            (true, true, true) => PipelineKind::PremultRoundedGray,
        };
        let shape = rounded.then(|| {
            Shape::new(
                sprite.bounds,
                radii(sprite.corner_radii),
                radii(sprite.corner_radii),
                Edges::default(),
                transparent_black(),
                BorderStyle::Solid,
                uv,
            )
        });
        self.append(
            draws,
            Rectangle {
                bounds: sprite.bounds,
                uv,
                color: white().opacity(sprite.opacity),
                transform: transform(origin),
            },
            local_mask(sprite.content_mask.bounds, origin),
            viewport,
            Some(texture),
            kind,
            shape,
        );
    }

    fn tile(&mut self, layer: &LayerFrame, tile: gpui::TileCoord, depth: usize) -> Option<Target> {
        let target = Target::Tile {
            layer: layer.key.0,
            x: tile.x,
            y: tile.y,
        };
        if !self.active.insert(target) {
            self.warn("cyclic layer references");
            return None;
        }
        let side = layer.tile_size;
        if side == 0 || side > self.device.limits().max_texture_dimension_2d {
            self.active.remove(&target);
            return None;
        }
        let created = self.targets.ensure(self.device, target, [side; 2]);
        let entry = self.targets.items.get_mut(&target).unwrap();
        let changed_content = entry
            .content
            .as_ref()
            .is_none_or(|content| !content.ptr_eq(&layer.content));
        let repaint = created
            || entry.background != layer.background
            || (entry.generation != layer.generation
                && (entry.generation.wrapping_add(1) != layer.generation
                    || layer.dirty_tiles.contains(&tile)))
            || (entry.generation == layer.generation && changed_content);
        entry.generation = layer.generation;
        entry.content = Some(layer.content.clone());
        entry.background = layer.background;
        if repaint {
            let scene = layer.tile_scene(tile);
            let draws = self.scene(
                &scene,
                [side; 2],
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                depth,
            );
            self.plan.passes.push(Pass {
                target: Some(target),
                clear: clear(layer.background),
                draws,
            });
        }
        self.active.remove(&target);
        Some(target)
    }

    fn group(
        &mut self,
        draws: &mut Vec<Draw>,
        group: &LayerFrame,
        sprite: &PolychromeSprite,
        viewport: [u32; 2],
        origin: Point<ScaledPixels>,
        depth: usize,
    ) {
        let Some(child) = group.content.scene() else {
            return;
        };
        let bounds = local_mask(sprite.bounds, origin);
        let mask = local_mask(sprite.content_mask.bounds, origin);
        let left = bounds.origin.x.0.max(mask.origin.x.0).max(0.0).floor();
        let top = bounds.origin.y.0.max(mask.origin.y.0).max(0.0).floor();
        let right = bounds
            .right()
            .0
            .min(mask.right().0)
            .min(viewport[0] as f32)
            .ceil();
        let bottom = bounds
            .bottom()
            .0
            .min(mask.bottom().0)
            .min(viewport[1] as f32)
            .ceil();
        if right <= left || bottom <= top {
            return;
        }
        let extent = [(right - left) as u32, (bottom - top) as u32];
        let target = Target::Group(self.next_group);
        self.next_group += 1;
        self.targets.ensure(self.device, target, extent);
        let child_origin = origin + point(ScaledPixels(left), ScaledPixels(top));
        let child_draws = self.scene(child, extent, child_origin, depth);
        self.plan.passes.push(Pass {
            target: Some(target),
            clear: wgpu::Color::TRANSPARENT,
            draws: child_draws,
        });
        let rect = Bounds {
            origin: child_origin,
            size: size(
                ScaledPixels(extent[0] as f32),
                ScaledPixels(extent[1] as f32),
            ),
        };
        let vertical = child
            .layers
            .frames
            .iter()
            .find(|frame| frame.key.0 == u32::MAX)
            .map(|frame| frame.background);
        let ry = vertical
            .map(|v| [v.r, v.g, v.b, v.a])
            .unwrap_or_else(|| radii(sprite.corner_radii));
        let mut shape = Shape::new(
            sprite.bounds,
            radii(sprite.corner_radii),
            ry,
            Edges::default(),
            transparent_black(),
            BorderStyle::Solid,
            [0.0, 0.0, 1.0, 1.0],
        );
        shape.mapping = [
            extent[0] as f32,
            extent[1] as f32,
            child_origin.x.0 - sprite.bounds.origin.x.0,
            child_origin.y.0 - sprite.bounds.origin.y.0,
        ];
        self.append(
            draws,
            Rectangle {
                bounds: rect,
                uv: [0.0, 0.0, 1.0, 1.0],
                color: white().opacity(sprite.opacity),
                transform: transform(origin),
            },
            mask,
            viewport,
            Some(Texture::Target(target)),
            PipelineKind::PremultRounded,
            Some(shape),
        );
    }
}
