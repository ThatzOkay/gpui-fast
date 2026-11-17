mod geometry;
mod scene;
mod shapes;
#[cfg(test)]
mod tests;

use super::{GpuContext, RendererState, WgpuRenderer, WgpuSurfaceConfig};
use crate::{CompositorGpuHint, WgpuAtlas, WgpuContext};
use geometry::Vertex;
use gpui::{GpuSpecs, Scene};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PipelineKind {
    SolidOpaque,
    SolidAlpha,
    Monochrome,
    Rounded,
    Border,
    BorderDashed,
    Color,
    ColorGray,
    ColorRounded,
    ColorRoundedGray,
    Premult,
    PremultGray,
    PremultRounded,
    PremultRoundedGray,
}
impl PipelineKind {
    fn needs_shape(self) -> bool {
        matches!(
            self,
            Self::Rounded
                | Self::Border
                | Self::BorderDashed
                | Self::ColorRounded
                | Self::ColorRoundedGray
                | Self::PremultRounded
                | Self::PremultRoundedGray
        )
    }
}
struct Draw {
    kind: PipelineKind,
    scissor: [u32; 4],
    vertices: Range<u32>,
    texture: Option<scene::Texture>,
    shape: Option<usize>,
}

pub(super) struct Renderer {
    pub surface: Option<wgpu::Surface<'static>>,
    context: WgpuContext,
    atlas: Arc<WgpuAtlas>,
    pipelines: HashMap<(PipelineKind, bool), wgpu::RenderPipeline>,
    shape_layout: wgpu::BindGroupLayout,
    targets: scene::Targets,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_capacity: u64,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    warned: HashSet<&'static str>,
}

impl Renderer {
    fn new(
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        atlas: Arc<WgpuAtlas>,
        format: wgpu::TextureFormat,
    ) -> Self {
        let device = &context.device;
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gles2 texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gles2 pipeline layout"),
            bind_group_layouts: &[Some(&texture_layout)],
            immediate_size: 0,
        });
        let shape_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gles2 shape layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(96),
                },
                count: None,
            }],
        });
        let rounded_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gles2 rounded layout"),
            bind_group_layouts: &[Some(&texture_layout), Some(&shape_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gles2 primitives"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let create_pipeline = |label, entry_point, kind: PipelineKind, target_format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(if kind.needs_shape() {
                    &rounded_layout
                } else {
                    &layout
                }),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: if kind == PipelineKind::SolidOpaque {
                            None
                        } else {
                            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                        },
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let mut pipelines = HashMap::new();
        for (kind, entry) in [
            (PipelineKind::SolidOpaque, "fs_solid"),
            (PipelineKind::SolidAlpha, "fs_solid"),
            (PipelineKind::Monochrome, "fs_mono"),
            (PipelineKind::Rounded, "fs_rounded"),
            (PipelineKind::Border, "fs_border"),
            (PipelineKind::BorderDashed, "fs_border_dashed"),
            (PipelineKind::Color, "fs_color"),
            (PipelineKind::ColorGray, "fs_color_gray"),
            (PipelineKind::ColorRounded, "fs_color_rounded"),
            (PipelineKind::ColorRoundedGray, "fs_color_rounded_gray"),
            (PipelineKind::Premult, "fs_premult"),
            (PipelineKind::PremultGray, "fs_premult_gray"),
            (PipelineKind::PremultRounded, "fs_premult_rounded"),
            (PipelineKind::PremultRoundedGray, "fs_premult_rounded_gray"),
        ] {
            for offscreen in [false, true] {
                let target = if offscreen {
                    wgpu::TextureFormat::Rgba8Unorm
                } else {
                    format
                };
                pipelines.insert(
                    (kind, offscreen),
                    create_pipeline(entry, entry, kind, target),
                );
            }
        }
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gles2 white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        context.queue.write_texture(
            white.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            white.size(),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gles2 atlas sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        log::info!("Using GLES 2 scene renderer (quads, borders, sprites and retained layers)");
        Self {
            surface: Some(surface),
            context: context.clone(),
            atlas,
            pipelines,
            shape_layout,
            targets: Default::default(),
            vertex_buffer: None,
            vertex_capacity: 0,
            texture_layout,
            sampler,
            white: white.create_view(&Default::default()),
            warned: HashSet::new(),
        }
    }

    pub fn configure(&self, config: &wgpu::SurfaceConfiguration) {
        if let Some(surface) = &self.surface {
            surface.configure(&self.context.device, config);
        }
    }

    pub fn gpu_specs(&self) -> GpuSpecs {
        let info = self.context.adapter.get_info();
        GpuSpecs {
            is_software_emulated: info.device_type == wgpu::DeviceType::Cpu,
            device_name: info.name,
            driver_name: info.driver,
            driver_info: info.driver_info,
        }
    }

    pub fn draw(&mut self, scene: &Scene, config: &wgpu::SurfaceConfiguration) -> bool {
        let Some(surface) = &self.surface else {
            return false;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.configure(config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure(config);
                return false;
            }
            other => {
                log::warn!("GLES 2 surface acquisition: {other:?}");
                return false;
            }
        };
        let plan = scene::plan(
            scene,
            [frame.texture.width(), frame.texture.height()],
            &self.context.device,
            &self.atlas,
            &mut self.targets,
            &mut self.warned,
        );
        let vertices = &plan.vertices;
        self.atlas.before_frame();
        let device = &self.context.device;
        let bindings: Vec<Vec<_>> = plan
            .passes
            .iter()
            .map(|pass| {
                pass.draws
                    .iter()
                    .map(|draw| {
                        let info = match draw.texture {
                            Some(scene::Texture::Atlas(id)) => self.atlas.get_texture_info(id),
                            _ => None,
                        };
                        let view = match draw.texture {
                            Some(scene::Texture::Target(key)) => self.targets.view(key),
                            _ => info.as_ref().map(|info| &info.view).unwrap_or(&self.white),
                        };
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("gles2 texture"),
                            layout: &self.texture_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                                },
                            ],
                        })
                    })
                    .collect()
            })
            .collect();
        let stride = 96_u64
            .div_ceil(device.limits().min_uniform_buffer_offset_alignment.max(16) as u64)
            * device.limits().min_uniform_buffer_offset_alignment.max(16) as u64;
        let shape_buffer = (!plan.shapes.is_empty()).then(|| {
            let mut bytes = vec![0; stride as usize * plan.shapes.len()];
            for (index, shape) in plan.shapes.iter().enumerate() {
                bytes[index * stride as usize..index * stride as usize + 96]
                    .copy_from_slice(bytemuck::bytes_of(shape));
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gles2 shapes"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.context.queue.write_buffer(&buffer, 0, &bytes);
            buffer
        });
        let shapes: Vec<_> = plan
            .shapes
            .iter()
            .enumerate()
            .map(|(index, _)| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gles2 shape"),
                    layout: &self.shape_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: shape_buffer.as_ref().unwrap(),
                            offset: index as u64 * stride,
                            size: std::num::NonZeroU64::new(96),
                        }),
                    }],
                })
            })
            .collect();
        if !vertices.is_empty() {
            let bytes = bytemuck::cast_slice(vertices);
            if bytes.len() as u64 > self.vertex_capacity {
                self.vertex_capacity = (bytes.len() as u64)
                    .next_power_of_two()
                    .min(device.limits().max_buffer_size);
                self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gles2 vertices"),
                    size: self.vertex_capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            self.context
                .queue
                .write_buffer(self.vertex_buffer.as_ref().unwrap(), 0, bytes);
        }
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        for (planned, bindings) in plan.passes.iter().zip(&bindings) {
            let target = planned
                .target
                .map(|key| self.targets.view(key))
                .unwrap_or(&view);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gles2 scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(planned.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !vertices.is_empty() {
                pass.set_vertex_buffer(0, self.vertex_buffer.as_ref().unwrap().slice(..));
                let mut pipeline = None;
                for (draw, binding) in planned.draws.iter().zip(bindings) {
                    if pipeline != Some(draw.kind) {
                        pass.set_pipeline(&self.pipelines[&(draw.kind, planned.target.is_some())]);
                        pipeline = Some(draw.kind);
                    }
                    let [x, y, width, height] = draw.scissor;
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_bind_group(0, binding, &[]);
                    if let Some(shape) = draw.shape {
                        pass.set_bind_group(1, &shapes[shape], &[]);
                    }
                    pass.draw(draw.vertices.clone(), 0..1);
                }
            }
        }
        self.context.queue.submit(Some(encoder.finish()));
        self.context.queue.present(frame);
        true
    }
}

impl WgpuRenderer {
    pub(super) fn new_gles2(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
        surface_configured: bool,
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
    ) -> anyhow::Result<Self> {
        let (surface_config, transparent_alpha_mode, opaque_alpha_mode) =
            Self::surface_configuration(&surface, &context.adapter, &context.device, &config)?;
        if !surface_configured {
            surface.configure(&context.device, &surface_config);
        }
        let renderer = Renderer::new(context, surface, atlas.clone(), surface_config.format);
        Ok(Self {
            fast_gles2: Some(renderer),
            context: gpu_context,
            compositor_gpu,
            state: RendererState::Released,
            surface_config,
            atlas,
            transparent_alpha_mode,
            opaque_alpha_mode,
            max_texture_size: context.device.limits().max_texture_dimension_2d,
            is_bgr: false,
            failed_frame_count: 0,
            device_errors: context.errors().clone(),
            observed_error_generation: 0,
            last_surface_error: None,
            needs_redraw: false,
        })
    }
}
