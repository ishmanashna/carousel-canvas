use std::collections::HashMap;

use bytemuck;
use image::{RgbaImage, RgbImage};
use wgpu::util::DeviceExt;

use crate::card_edge::{split_horizontal_chunks, RenderCard, MAX_GPU_TEXTURE_DIM};
use crate::color::parse_color_rgb;
use crate::error::{DecodeError, Result};
use crate::polaroid::{polaroid_shader_source, polaroid_uniforms, polaroid_vertices, PolaroidDrawMode, PolaroidUniforms, Vertex as PolaroidVertex};

const SHADER: &str = r#"
struct Uniforms { view_proj: mat4x4<f32>, }
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var card_tex: texture_2d<f32>;
@group(1) @binding(1) var card_sampler: sampler;
struct VertexInput { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, }
struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) uv: vec2<f32>, }
@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(vertex.pos, 0.0, 1.0);
    out.uv = vertex.uv;
    return out;
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(card_tex, card_sampler, in.uv);
}
@fragment
fn fs_shadow(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(card_tex, card_sampler, in.uv);
    return vec4<f32>(0.0, 0.0, 0.0, tex.a * 0.45);
}
"#;

const SHADOW_OFFSET_X: f32 = 12.0;
const SHADOW_OFFSET_Y: f32 = 16.0;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex { pos: [f32; 2], uv: [f32; 2], }

#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms { view_proj: [[f32; 4]; 4], }

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}
unsafe impl bytemuck::Pod for Uniforms {}
unsafe impl bytemuck::Zeroable for Uniforms {}

struct GpuCardTexture { bind_group: wgpu::BindGroup }

struct GpuBgTexture { bind_group: wgpu::BindGroup }

pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    polaroid_pipeline: wgpu::RenderPipeline,
    polaroid_uniform_buffer: wgpu::Buffer,
    polaroid_uniform_bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl Compositor {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("card_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_bgl"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("card_pipeline_layout"),
            bind_group_layouts: &[&uniform_bgl, &texture_bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("card_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("card_shadow_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_shadow"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view_proj_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bg"),
            layout: &uniform_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
        });

        let polaroid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("polaroid_shader"),
            source: wgpu::ShaderSource::Wgsl(polaroid_shader_source().into()),
        });
        let polaroid_uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("polaroid_uniform_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let polaroid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("polaroid_pipeline_layout"),
            bind_group_layouts: &[&polaroid_uniform_bgl, &texture_bgl],
            push_constant_ranges: &[],
        });
        let polaroid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("polaroid_pipeline"),
            layout: Some(&polaroid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &polaroid_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PolaroidVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &polaroid_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let polaroid_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("polaroid_uniform"),
            size: std::mem::size_of::<PolaroidUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let polaroid_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("polaroid_uniform_bg"),
            layout: &polaroid_uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: polaroid_uniform_buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("card_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            device,
            queue,
            pipeline,
            shadow_pipeline,
            uniform_buffer,
            uniform_bind_group,
            polaroid_pipeline,
            polaroid_uniform_buffer,
            polaroid_uniform_bind_group,
            sampler,
            bind_group_layout: texture_bgl,
        }
    }

    fn upload_bg_texture(&self, image: &RgbImage) -> GpuBgTexture {
        let w = image.width().max(1);
        let h = image.height().max(1);
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        for (i, px) in image.pixels().enumerate() {
            let o = i * 4;
            rgba[o] = px[0];
            rgba[o + 1] = px[1];
            rgba[o + 2] = px[2];
            rgba[o + 3] = 255;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bg_tex"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        GpuBgTexture { bind_group }
    }

    fn draw_background_quad(
        pass: &mut wgpu::RenderPass<'_>,
        bg: &GpuBgTexture,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        device: &wgpu::Device,
    ) {
        let verts = [
            Vertex { pos: [left, top], uv: [0.0, 0.0] },
            Vertex { pos: [right, top], uv: [1.0, 0.0] },
            Vertex { pos: [right, bottom], uv: [1.0, 1.0] },
            Vertex { pos: [left, bottom], uv: [0.0, 1.0] },
        ];
        let tri = [
            verts[0], verts[1], verts[2],
            verts[0], verts[2], verts[3],
        ];
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bg_verts"),
            contents: bytemuck::cast_slice(&tri),
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_bind_group(1, &bg.bind_group, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_textured_quad_shadow(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu_tex: &GpuCardTexture,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
    ) {
        let offset_corners = offset_corners(corners, SHADOW_OFFSET_X, SHADOW_OFFSET_Y);
        let verts = [
            Vertex { pos: offset_corners[0], uv: uvs[0] },
            Vertex { pos: offset_corners[1], uv: uvs[1] },
            Vertex { pos: offset_corners[2], uv: uvs[2] },
            Vertex { pos: offset_corners[0], uv: uvs[0] },
            Vertex { pos: offset_corners[2], uv: uvs[2] },
            Vertex { pos: offset_corners[3], uv: uvs[3] },
        ];
        let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("card_shadow_verts"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_bind_group(1, &gpu_tex.bind_group, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_textured_quad(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu_tex: &GpuCardTexture,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
    ) {
        let verts = [
            Vertex { pos: corners[0], uv: uvs[0] },
            Vertex { pos: corners[1], uv: uvs[1] },
            Vertex { pos: corners[2], uv: uvs[2] },
            Vertex { pos: corners[0], uv: uvs[0] },
            Vertex { pos: corners[2], uv: uvs[2] },
            Vertex { pos: corners[3], uv: uvs[3] },
        ];
        let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("card_verts"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_bind_group(1, &gpu_tex.bind_group, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_polaroid_card(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        card: &RenderCard,
        gpu_tex: &GpuCardTexture,
        view_proj: [[f32; 4]; 4],
    ) {
        let scene_card = polaroid_scene_card(card);
        for mode in [
            PolaroidDrawMode::Reflection,
            PolaroidDrawMode::Shadow,
            PolaroidDrawMode::Card,
        ] {
            let uniforms = polaroid_uniforms(view_proj, &scene_card, mode);
            self.queue.write_buffer(
                &self.polaroid_uniform_buffer,
                0,
                bytemuck::bytes_of(&uniforms),
            );
            let verts = polaroid_vertices(&scene_card, mode);
            let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("polaroid_verts"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            pass.set_pipeline(&self.polaroid_pipeline);
            pass.set_bind_group(0, &self.polaroid_uniform_bind_group, &[]);
            pass.set_bind_group(1, &gpu_tex.bind_group, &[]);
            pass.set_vertex_buffer(0, vbuf.slice(..));
            pass.draw(0..6, 0..1);
        }
    }

    fn draw_render_card(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        card: &RenderCard,
        textures: &HashMap<usize, GpuCardTexture>,
        view_proj: [[f32; 4]; 4],
        card_index: usize,
    ) {
        let Some(gpu_tex) = textures.get(&card_index) else {
            return;
        };
        if card.polaroid {
            self.draw_polaroid_card(pass, card, gpu_tex, view_proj);
            return;
        }
        if card.rotation_deg.abs() < 1e-6 && card.image.width() > MAX_GPU_TEXTURE_DIM {
            let chunks = split_horizontal_chunks(&card.image, MAX_GPU_TEXTURE_DIM);
            let img_w = card.image.width() as f32;
            let img_h = card.image.height() as f32;
            let left = card.center_x - img_w * 0.5;
            let top = card.center_y - img_h * 0.5;
            for (i, (x0, chunk)) in chunks.iter().enumerate() {
                let tex_key = card_index * 100 + i;
                let Some(chunk_tex) = textures.get(&tex_key) else {
                    continue;
                };
                let cw = chunk.width() as f32;
                let ch = chunk.height() as f32;
                let x = left + *x0 as f32;
                let corners = [
                    [x, top],
                    [x + cw, top],
                    [x + cw, top + ch],
                    [x, top + ch],
                ];
                let uvs = unit_uvs();
                if card.cast_shadow {
                    self.draw_textured_quad_shadow(pass, chunk_tex, corners, uvs);
                }
                self.draw_textured_quad(pass, chunk_tex, corners, uvs);
            }
            return;
        }
        let (corners, uvs) = render_card_corners(card);
        if card.cast_shadow {
            self.draw_textured_quad_shadow(pass, gpu_tex, corners, uvs);
        }
        self.draw_textured_quad(pass, gpu_tex, corners, uvs);
    }

    fn upload_texture(&self, image: &RgbaImage) -> GpuCardTexture {
        let w = image.width().max(1);
        let h = image.height().max(1);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("card_tex"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("card_bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        GpuCardTexture { bind_group }
    }

    fn upload_render_cards(&self, cards: &[RenderCard]) -> HashMap<usize, GpuCardTexture> {
        let mut map = HashMap::new();
        for (i, card) in cards.iter().enumerate() {
            if card.rotation_deg.abs() < 1e-6 && card.image.width() > MAX_GPU_TEXTURE_DIM {
                for (j, (_x0, chunk)) in
                    split_horizontal_chunks(&card.image, MAX_GPU_TEXTURE_DIM)
                        .into_iter()
                        .enumerate()
                {
                    map.insert(i * 100 + j, self.upload_texture(&chunk));
                }
            } else {
                map.insert(i, self.upload_texture(&card.image));
            }
        }
        map
    }

    pub fn render_all_tiles(
        &self,
        cards: &[RenderCard],
        background: &str,
        background_rgb: Option<&RgbImage>,
        slice_count: usize,
        slice_w: u32,
        slice_h: u32,
        bleed_px: i32,
    ) -> Result<Vec<RgbImage>> {
        let textures = self.upload_render_cards(cards);
        let mut out = Vec::with_capacity(slice_count);
        for i in 0..slice_count {
            out.push(
                self.render_tile_rgb(
                    cards,
                    background,
                    &textures,
                    background_rgb,
                    i,
                    slice_w,
                    slice_h,
                    bleed_px,
                )?,
            );
        }
        Ok(out)
    }

    /// Full-canvas RGB flatten of required slots only (for underfill planner).
    pub fn render_required_slots_flat_rgb(
        &self,
        cards: &[RenderCard],
        background: &str,
        canvas_width: i32,
        canvas_height: i32,
    ) -> Result<Vec<u8>> {
        let cw = canvas_width.max(1) as u32;
        let ch = canvas_height.max(1) as u32;
        let rgba = self.render_canvas_rgba(cards, background, cw, ch)?;
        let bg = parse_color_rgb(background);
        Ok(flatten_rgba_to_rgb(&rgba, cw, ch, bg))
    }

    /// Full-canvas RGB render (preview / wide master at arbitrary resolution).
    pub fn render_scene_to_rgb(
        &self,
        cards: &[RenderCard],
        background: &str,
        background_rgb: Option<&RgbImage>,
        width: u32,
        height: u32,
    ) -> Result<RgbImage> {
        let textures = self.upload_render_cards(cards);
        let rgba = self.render_canvas_rgba_region_with_bg(
            cards,
            background,
            &textures,
            background_rgb,
            0.0,
            width as f32,
            0.0,
            height as f32,
            width,
            height,
        )?;
        let bg = parse_color_rgb(background);
        Ok(rgba_to_rgb_image(&rgba, bg))
    }

    fn render_canvas_rgba(
        &self,
        cards: &[RenderCard],
        background: &str,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage> {
        const MAX_TEX: u32 = 8192;
        const OVERLAP: i32 = 1400;
        let textures = self.upload_render_cards(cards);
        if width <= MAX_TEX {
            return self.render_canvas_rgba_region_with_bg(
                cards,
                background,
                &textures,
                None,
                0.0,
                width as f32,
                0.0,
                height as f32,
                width,
                height,
            );
        }
        let mut full = RgbaImage::new(width, height);
        let split = (width / 2) as i32;
        let left_end = (split + OVERLAP).min(width as i32);
        let right_start = (split - OVERLAP).max(0);
        let left_w = left_end as u32;
        let left_img = self.render_canvas_rgba_region_with_bg(
            cards,
            background,
            &textures,
            None,
            0.0,
            left_end as f32,
            0.0,
            height as f32,
            left_w,
            height,
        )?;
        for y in 0..height {
            for x in 0..split as u32 {
                full.put_pixel(x, y, *left_img.get_pixel(x, y));
            }
        }
        let right_w = (width as i32 - right_start) as u32;
        let right_img = self.render_canvas_rgba_region_with_bg(
            cards,
            background,
            &textures,
            None,
            right_start as f32,
            width as f32,
            0.0,
            height as f32,
            right_w,
            height,
        )?;
        for y in 0..height {
            for x in 0..right_w {
                let cx = right_start + x as i32;
                if cx >= split {
                    full.put_pixel(cx as u32, y, *right_img.get_pixel(x, y));
                }
            }
        }
        Ok(full)
    }

    fn render_canvas_rgba_region_with_bg(
        &self,
        cards: &[RenderCard],
        background: &str,
        textures: &HashMap<usize, GpuCardTexture>,
        background_rgb: Option<&RgbImage>,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage> {
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let bg = parse_color_rgb(background);
        let bg_color = wgpu::Color {
            r: bg[0] as f64 / 255.0,
            g: bg[1] as f64 / 255.0,
            b: bg[2] as f64 / 255.0,
            a: 1.0,
        };
        let view_proj = ortho_y_down(left, right, top, bottom);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniforms { view_proj }),
        );
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("canvas_encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(bg_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            if let Some(bg_full) = background_rgb {
                let crop = Self::crop_background_region(bg_full, left, top, right, bottom);
                let bg_tex = self.upload_bg_texture(&crop);
                Self::draw_background_quad(
                    &mut pass,
                    &bg_tex,
                    left,
                    right,
                    top,
                    bottom,
                    &self.device,
                );
            }
            for (card_i, card) in cards.iter().enumerate() {
                self.draw_render_card(&mut pass, card, textures, view_proj, card_i);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        read_texture_rgba(&self.device, &self.queue, &target, width, height)
    }

    fn crop_background_region(
        bg: &RgbImage,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    ) -> RgbImage {
        let cw = bg.width();
        let ch = bg.height();
        let x0 = (left.floor() as i32).max(0).min(cw as i32) as u32;
        let y0 = (top.floor() as i32).max(0).min(ch as i32) as u32;
        let x1 = (right.ceil() as i32).max(0).min(cw as i32) as u32;
        let y1 = (bottom.ceil() as i32).max(0).min(ch as i32) as u32;
        let w = x1.saturating_sub(x0).max(1);
        let h = y1.saturating_sub(y0).max(1);
        image::imageops::crop_imm(bg, x0, y0, w, h).to_image()
    }

    fn render_tile_rgb(
        &self,
        cards: &[RenderCard],
        background: &str,
        textures: &HashMap<usize, GpuCardTexture>,
        background_rgb: Option<&RgbImage>,
        slice_index: usize,
        slice_w: u32,
        slice_h: u32,
        bleed_px: i32,
    ) -> Result<RgbImage> {
        let render_w = (slice_w as i32 + 2 * bleed_px).max(1) as u32;
        let render_h = (slice_h as i32 + 2 * bleed_px).max(1) as u32;
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tile_target"),
            size: wgpu::Extent3d { width: render_w, height: render_h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let bg = parse_color_rgb(background);
        let bg_color = wgpu::Color {
            r: bg[0] as f64 / 255.0,
            g: bg[1] as f64 / 255.0,
            b: bg[2] as f64 / 255.0,
            a: 1.0,
        };
        let left = slice_index as f32 * slice_w as f32 - bleed_px as f32;
        let right = (slice_index as f32 + 1.0) * slice_w as f32 + bleed_px as f32;
        let top = -(bleed_px as f32);
        let bottom = slice_h as f32 + bleed_px as f32;
        let view_proj = ortho_y_down(left, right, top, bottom);
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&Uniforms {
            view_proj,
        }));
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tile_encoder") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tile_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(bg_color), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            if let Some(bg_full) = background_rgb {
                let crop = Self::crop_background_region(bg_full, left, top, right, bottom);
                let bg_tex = self.upload_bg_texture(&crop);
                Self::draw_background_quad(
                    &mut pass,
                    &bg_tex,
                    left,
                    right,
                    top,
                    bottom,
                    &self.device,
                );
            }
            for (card_i, card) in cards.iter().enumerate() {
                self.draw_render_card(&mut pass, card, textures, view_proj, card_i);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        let rgba = read_texture_rgba(&self.device, &self.queue, &target, render_w, render_h)?;
        crop_center_rgb(&rgba, render_w, render_h, slice_w, slice_h, bleed_px)
    }
}

fn rgba_to_rgb_image(rgba: &RgbaImage, bg: [u8; 3]) -> RgbImage {
    let cw = rgba.width();
    let ch = rgba.height();
    let raw = flatten_rgba_to_rgb(rgba, cw, ch, bg);
    RgbImage::from_raw(cw, ch, raw).expect("rgb buffer size")
}

fn flatten_rgba_to_rgb(rgba: &RgbaImage, cw: u32, ch: u32, bg: [u8; 3]) -> Vec<u8> {
    let mut rgb = vec![0u8; cw as usize * ch as usize * 3];
    for y in 0..ch {
        for x in 0..cw {
            let px = rgba.get_pixel(x, y);
            let a = px[3] as f32 / 255.0;
            let idx = (y as usize * cw as usize + x as usize) * 3;
            if a <= 0.001 {
                rgb[idx] = bg[0];
                rgb[idx + 1] = bg[1];
                rgb[idx + 2] = bg[2];
            } else {
                rgb[idx] = ((px[0] as f32 * a) + bg[0] as f32 * (1.0 - a)).round() as u8;
                rgb[idx + 1] = ((px[1] as f32 * a) + bg[1] as f32 * (1.0 - a)).round() as u8;
                rgb[idx + 2] = ((px[2] as f32 * a) + bg[2] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }
    rgb
}

fn ortho_y_down(left: f32, right: f32, top: f32, bottom: f32) -> [[f32; 4]; 4] {
    let w = right - left;
    let h = bottom - top;
    [[2.0 / w, 0.0, 0.0, 0.0], [0.0, -2.0 / h, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-(right + left) / w, (bottom + top) / h, 0.0, 1.0]]
}

fn rotate_point(x: f32, y: f32, cx: f32, cy: f32, deg: f32) -> [f32; 2] {
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let dx = x - cx;
    let dy = y - cy;
    [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
}

fn unit_uvs() -> [[f32; 2]; 4] {
    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
}

fn offset_corners(corners: [[f32; 2]; 4], dx: f32, dy: f32) -> [[f32; 2]; 4] {
    corners.map(|[x, y]| [x + dx, y + dy])
}

fn polaroid_scene_card(card: &RenderCard) -> core::SceneCard {
    core::SceneCard {
        photo_path: std::path::PathBuf::new(),
        dest: core::Rect {
            x: (card.center_x - card.dest_w as f32 * 0.5).round() as i32,
            y: (card.center_y - card.dest_h as f32 * 0.5).round() as i32,
            w: card.dest_w,
            h: card.dest_h,
        },
        z: card.z,
        rotation_deg: card.rotation_deg as f64,
        fit: core::Fit::Cover,
        pan_x: 0.0,
        pan_y: 0.0,
        flip_h: false,
        source_trim_left_frac: None,
        horizontal_center_band_frac: None,
        cover_height_first: false,
        polaroid: true,
        slot_seed: card.slot_seed,
        cutout: false,
        mask_path: None,
        source_crop: None,
        cast_shadow: false,
        edge_feather_px: 0,
    }
}

fn render_card_corners(card: &RenderCard) -> ([[f32; 2]; 4], [[f32; 2]; 4]) {
    if card.rotation_deg.abs() < 1e-6 {
        let w = card.image.width() as f32;
        let h = card.image.height() as f32;
        let left = card.center_x - w * 0.5;
        let top = card.center_y - h * 0.5;
        let corners = [
            [left, top],
            [left + w, top],
            [left + w, top + h],
            [left, top + h],
        ];
        return (corners, unit_uvs());
    }
    let x = card.center_x - card.dest_w as f32 * 0.5;
    let y = card.center_y - card.dest_h as f32 * 0.5;
    let w = card.dest_w.max(1) as f32;
    let h = card.dest_h.max(1) as f32;
    let cx = card.center_x;
    let cy = card.center_y;
    let corners = [
        rotate_point(x, y, cx, cy, card.rotation_deg),
        rotate_point(x + w, y, cx, cy, card.rotation_deg),
        rotate_point(x + w, y + h, cx, cy, card.rotation_deg),
        rotate_point(x, y + h, cx, cy, card.rotation_deg),
    ];
    (corners, unit_uvs())
}

fn align_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    (unpadded + wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1) / wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

fn read_texture_rgba(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, width: u32, height: u32) -> Result<RgbaImage> {
    let bytes_per_row = align_bytes_per_row(width);
    let padded = (bytes_per_row * height) as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: padded,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback_encoder") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().map_err(|_| DecodeError::Gpu("readback channel closed".into()))?
        .map_err(|e| DecodeError::Gpu(format!("map_async: {e:?}")))?;
    let data = slice.get_mapped_range();
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for row in 0..height {
        let src = (row * bytes_per_row) as usize;
        let dst = (row * width * 4) as usize;
        pixels[dst..dst + (width * 4) as usize].copy_from_slice(&data[src..src + (width * 4) as usize]);
    }
    drop(data);
    buffer.unmap();
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| DecodeError::Gpu("invalid readback dimensions".into()))
}

fn crop_center_rgb(rgba: &RgbaImage, _render_w: u32, _render_h: u32, slice_w: u32, slice_h: u32, bleed_px: i32) -> Result<RgbImage> {
    let x0 = bleed_px.max(0) as u32;
    let y0 = bleed_px.max(0) as u32;
    let mut rgb = RgbImage::new(slice_w, slice_h);
    for y in 0..slice_h {
        for x in 0..slice_w {
            let px = rgba.get_pixel(x0 + x, y0 + y);
            rgb.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
        }
    }
    Ok(rgb)
}

pub fn tile_camera_rect(slice_index: usize, slice_w: i32, slice_h: i32, bleed_px: i32) -> (i32, i32, i32, i32) {
    (
        slice_index as i32 * slice_w - bleed_px,
        -bleed_px,
        (slice_index as i32 + 1) * slice_w + bleed_px,
        slice_h + bleed_px,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_edge::RenderCard;
    use crate::strip_export::create_offscreen_compositor;
    use image::Rgba;

    #[test]
    fn tile_camera_strip_10col() {
        assert_eq!(tile_camera_rect(0, 1080, 1350, 0), (0, 0, 1080, 1350));
        assert_eq!(tile_camera_rect(9, 1080, 1350, 0), (9720, 0, 10800, 1350));
    }

    fn solid_card(
        w: u32,
        h: u32,
        color: [u8; 4],
        cx: f32,
        cy: f32,
        z: i32,
        cast_shadow: bool,
    ) -> RenderCard {
        RenderCard {
            image: RgbaImage::from_pixel(w, h, Rgba(color)),
            z,
            rotation_deg: 0.0,
            center_x: cx,
            center_y: cy,
            polaroid: false,
            slot_seed: 0,
            dest_w: w as i32,
            dest_h: h as i32,
            cast_shadow,
        }
    }

    fn card_with_center_hole(w: u32, h: u32, fg: [u8; 4], cx: f32, cy: f32, z: i32) -> RenderCard {
        let mut img = RgbaImage::from_pixel(w, h, Rgba(fg));
        let hole_r = (w.min(h) / 4) as i32;
        let hx = (w / 2) as i32;
        let hy = (h / 2) as i32;
        for y in 0..h {
            for x in 0..w {
                let dx = x as i32 - hx;
                let dy = y as i32 - hy;
                if dx * dx + dy * dy <= hole_r * hole_r {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
        RenderCard {
            image: img,
            z,
            rotation_deg: 0.0,
            center_x: cx,
            center_y: cy,
            polaroid: false,
            slot_seed: 0,
            dest_w: w as i32,
            dest_h: h as i32,
            cast_shadow: true,
        }
    }

    #[test]
    fn overlapping_cutout_shows_bottom_card_through_hole() {
        let compositor = create_offscreen_compositor().expect("gpu");
        let cards = [
            solid_card(80, 80, [220, 30, 30, 255], 60.0, 60.0, 0, false),
            card_with_center_hole(80, 80, [30, 200, 30, 255], 60.0, 60.0, 1),
        ];
        let rgba = compositor
            .render_scene_to_rgb(&cards, "#ffffff", None, 120, 120)
            .expect("render");
        let center = rgba.get_pixel(60, 60).0;
        assert!(
            center[0] > center[1] + 40,
            "hole should reveal red bottom card, got {:?}",
            center
        );
    }

    #[test]
    fn cutout_shadow_darkens_pixels_beside_card() {
        let compositor = create_offscreen_compositor().expect("gpu");
        let with_shadow = [solid_card(40, 40, [10, 10, 10, 255], 50.0, 50.0, 0, true)];
        let without_shadow = [solid_card(40, 40, [10, 10, 10, 255], 50.0, 50.0, 0, false)];
        let bg = "#f0f0f0";
        let shadowed = compositor
            .render_scene_to_rgb(&with_shadow, bg, None, 120, 120)
            .expect("render shadow");
        let plain = compositor
            .render_scene_to_rgb(&without_shadow, bg, None, 120, 120)
            .expect("render plain");
        // Just outside the card's right edge; shadow offset is (+12, +16).
        let sx = 72u32;
        let sy = 66u32;
        let shadow_px = shadowed.get_pixel(sx, sy).0;
        let plain_px = plain.get_pixel(sx, sy).0;
        let shadow_luma = shadow_px[0] as u32 + shadow_px[1] as u32 + shadow_px[2] as u32;
        let plain_luma = plain_px[0] as u32 + plain_px[1] as u32 + plain_px[2] as u32;
        assert!(
            shadow_luma < plain_luma,
            "shadow should darken bg beside card: shadow={shadow_px:?} plain={plain_px:?}"
        );
    }
}
