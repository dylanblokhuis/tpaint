use std::{borrow::Cow, collections::HashMap};

use beuk::{
    ash::vk::{
        self, BufferUsageFlags, CullModeFlags, DescriptorImageInfo, FrontFace, PolygonMode,
        PrimitiveTopology,
    },
    buffer::{Buffer, BufferDescriptor, MemoryLocation},
    ctx::RenderContext,
    graphics_pipeline::{
        BlendComponent, BlendFactor, BlendOperation, BlendState, DepthBiasState, DepthStencilState,
        Extent3d, FragmentState, GraphicsPipeline, GraphicsPipelineDescriptor, MultisampleState,
        PrimitiveState, PushConstantRange, StencilState, VertexAttribute, VertexBufferLayout,
        VertexState, VertexStepMode,
    },
    memory::ResourceHandle,
    shaders::{ShaderDescriptor, ShaderOptimization},
    smallvec::smallvec,
    texture::Texture,
};
use slab::Slab;
use tpaint::epaint::{self, emath::NumExt, ImageDelta, Primitive, TextureId, Vertex};

// #[repr(C, align(16))]
// #[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
// struct Vertex {
//     position: [f32; 2],
//     uv: [f32; 2],
//     color: [f32; 4],
// }

/// Uniform buffer used when rendering.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, align(16))]
struct UniformBuffer {
    screen_size_in_points: [f32; 2],
    // Uniform buffers need to be at least 16 bytes in WebGL.
    // See https://github.com/gfx-rs/wgpu/issues/2072
    _padding: [u32; 2],
}
impl PartialEq for UniformBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.screen_size_in_points == other.screen_size_in_points
    }
}

/// Information about the screen used for rendering.
pub struct ScreenDescriptor {
    /// Size of the window in physical pixels.
    pub size_in_pixels: [u32; 2],

    /// HiDPI scale factor (pixels per point).
    pub pixels_per_point: f32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PushConstants {
    texture_index: u32,
    screen_size: [f32; 2],
    _pad: u32,
}

impl ScreenDescriptor {
    /// size in "logical" points
    fn screen_size_in_points(&self) -> [f32; 2] {
        [
            self.size_in_pixels[0] as f32 / self.pixels_per_point,
            self.size_in_pixels[1] as f32 / self.pixels_per_point,
        ]
    }
}
pub struct Renderer {
    pipeline: ResourceHandle<GraphicsPipeline>,

    index_buffer: SlicedBuffer,
    vertex_buffer: SlicedBuffer,

    textures_to_index: HashMap<TextureId, usize>,
    textures: Slab<ResourceHandle<Texture>>,
}

struct SlicedBuffer {
    buffer: ResourceHandle<Buffer>,
    slices: Vec<std::ops::Range<usize>>,
    capacity: u64,
}

fn create_vertex_buffer(ctx: &RenderContext, capacity: u64) -> ResourceHandle<Buffer> {
    ctx.create_buffer(&BufferDescriptor {
        debug_name: "egui_vertex_buffer",
        location: MemoryLocation::CpuToGpu,
        usage: BufferUsageFlags::VERTEX_BUFFER | BufferUsageFlags::TRANSFER_DST,
        size: capacity,
    })
}

fn create_index_buffer(ctx: &RenderContext, capacity: u64) -> ResourceHandle<Buffer> {
    ctx.create_buffer(&BufferDescriptor {
        debug_name: "egui_index_buffer",
        location: MemoryLocation::CpuToGpu,
        usage: BufferUsageFlags::INDEX_BUFFER | BufferUsageFlags::TRANSFER_DST,
        size: capacity,
    })
}

impl Renderer {
    pub fn new(ctx: &RenderContext) -> Self {
        let swapchain = ctx.get_swapchain();

        let graphics_pipeline = ctx.create_graphics_pipeline(
            "tpaint",
            GraphicsPipelineDescriptor {
                vertex: VertexState {
                    shader: ctx.create_shader(ShaderDescriptor {
                        label: "tpaint_vertex",
                        kind: beuk::shaders::ShaderKind::Vertex,
                        entry_point: "main".into(),
                        source: include_str!("tpaint.vert").into(),
                        optimization: ShaderOptimization::None,
                        ..Default::default()
                    }),
                    buffers: smallvec![VertexBufferLayout {
                        array_stride: 5 * 4,
                        step_mode: VertexStepMode::Vertex,
                        attributes: smallvec![
                            VertexAttribute {
                                format: vk::Format::R32G32_SFLOAT,
                                offset: 0,
                                shader_location: 0,
                            },
                            VertexAttribute {
                                format: vk::Format::R32G32_SFLOAT,
                                offset: 8,
                                shader_location: 1,
                            },
                            VertexAttribute {
                                format: vk::Format::R32_UINT,
                                offset: 16,
                                shader_location: 2,
                            },
                        ],
                    }],
                },
                fragment: FragmentState {
                    color_attachment_formats: smallvec![swapchain.surface_format.format],
                    depth_attachment_format: swapchain.depth_image_format,
                    shader: ctx.create_shader(ShaderDescriptor {
                        label: "tpaint_fragment",
                        kind: beuk::shaders::ShaderKind::Fragment,
                        entry_point: "main".into(),
                        source: include_str!("tpaint.frag").into(),
                        optimization: ShaderOptimization::None,
                        ..Default::default()
                    }),
                },
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TRIANGLE_LIST,
                    unclipped_depth: false,
                    conservative: false,
                    cull_mode: CullModeFlags::NONE,
                    front_face: FrontFace::COUNTER_CLOCKWISE,
                    polygon_mode: PolygonMode::FILL,
                },
                depth_stencil: Some(DepthStencilState {
                    format: swapchain.depth_image_format,
                    depth_write_enabled: false,
                    depth_compare: beuk::graphics_pipeline::CompareFunction::Always,
                    bias: DepthBiasState::default(),
                    stencil: StencilState::default(),
                }),
                multisample: MultisampleState {
                    count: 1,
                    alpha_to_coverage_enabled: false,
                    mask: !0,
                },
                blend: vec![BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::OneMinusDstAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }],
                prepend_descriptor_sets: None,
                push_constant_range: Some(PushConstantRange {
                    offset: 0,
                    range: std::mem::size_of::<PushConstants>() as u32,
                    stages: beuk::graphics_pipeline::ShaderStages::AllGraphics,
                }),
                viewport: None,
            },
        );

        const VERTEX_BUFFER_START_CAPACITY: u64 = (std::mem::size_of::<Vertex>() * 1024) as _;
        const INDEX_BUFFER_START_CAPACITY: u64 = (std::mem::size_of::<u32>() * 1024 * 3) as _;

        Self {
            pipeline: graphics_pipeline,
            vertex_buffer: SlicedBuffer {
                buffer: create_vertex_buffer(ctx, VERTEX_BUFFER_START_CAPACITY),
                slices: Vec::with_capacity(64),
                capacity: VERTEX_BUFFER_START_CAPACITY,
            },
            index_buffer: SlicedBuffer {
                buffer: create_index_buffer(ctx, INDEX_BUFFER_START_CAPACITY),
                slices: Vec::with_capacity(64),
                capacity: INDEX_BUFFER_START_CAPACITY,
            },
            textures_to_index: HashMap::default(),
            textures: Slab::default(),
        }
    }

    pub fn update_texture(&mut self, ctx: &RenderContext, id: TextureId, image_delta: &ImageDelta) {
        let width = image_delta.image.width() as u32;
        let height = image_delta.image.height() as u32;

        let size = Extent3d {
            width,
            height,
            depth: 1,
        };

        println!("Updating texture {:?} with size {:?}", id, size);

        let data_color32 = match &image_delta.image {
            epaint::ImageData::Color(image) => {
                assert_eq!(
                    width as usize * height as usize,
                    image.pixels.len(),
                    "Mismatch between texture size and texel count"
                );
                Cow::Borrowed(&image.pixels)
            }
            epaint::ImageData::Font(image) => {
                assert_eq!(
                    width as usize * height as usize,
                    image.pixels.len(),
                    "Mismatch between texture size and texel count"
                );
                Cow::Owned(image.srgba_pixels(None).collect::<Vec<_>>())
            }
        };
        let data_bytes: &[u8] = bytemuck::cast_slice(data_color32.as_slice());

        let new_texture_handle = ctx.create_texture_with_data(
            "tpaint_tex",
            &vk::ImageCreateInfo::default()
                .array_layers(1)
                .extent(size.into())
                .flags(vk::ImageCreateFlags::empty())
                .format(vk::Format::R8G8B8A8_UNORM)
                .image_type(vk::ImageType::TYPE_2D)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .mip_levels(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(
                    vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::TRANSFER_DST
                        | vk::ImageUsageFlags::TRANSFER_SRC,
                ),
            data_bytes,
            0,
            false,
        );

        if let Some(pos) = image_delta.pos {
            // update the existing texture
            let texture_index = self
                .textures_to_index
                .get(&id)
                .expect("Tried to update a texture that has not been allocated yet.");
            let texture = self.textures.get(*texture_index).unwrap();
            let current_texture = ctx.texture_manager.get(texture).unwrap();
            let top_left = vk::Offset3D {
                x: pos[0] as i32,
                y: pos[1] as i32,
                z: 0,
            };
            let bottom_right = vk::Offset3D {
                x: pos[0] as i32 + image_delta.image.width() as i32,
                y: pos[1] as i32 + image_delta.image.height() as i32,
                z: 1,
            };

            let new_texture = ctx.texture_manager.get(&new_texture_handle).unwrap();
            let region = vk::ImageBlit {
                src_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                src_offsets: [
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: new_texture.extent.width as i32,
                        y: new_texture.extent.height as i32,
                        z: new_texture.extent.depth as i32,
                    },
                ],
                dst_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                dst_offsets: [top_left, bottom_right],
            };

            ctx.record_submit(|command_buffer| unsafe {
                println!(
                    "pos: {:?} {:?} {:?}",
                    pos, current_texture.extent, new_texture.extent
                );
                ctx.device.cmd_blit_image(
                    command_buffer,
                    new_texture.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    current_texture.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                    vk::Filter::NEAREST,
                );
            });
        } else {
            let mut pipeline = ctx.graphics_pipelines.get(&self.pipeline).unwrap();
            let index = self.textures.insert(new_texture_handle.clone());

            pipeline.queue_descriptor_image(
                0,
                0,
                index as u32,
                DescriptorImageInfo::default()
                    .image_view(*ctx.get_texture_view(&new_texture_handle).unwrap())
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            );
            pipeline.update_descriptors(ctx);

            self.textures_to_index.insert(id, index);
        }
    }

    pub fn free_texture(&mut self, id: &epaint::TextureId) {
        let index = self.textures_to_index.remove(id).unwrap();
        self.textures.remove(index);
    }

    /// Get the WGPU texture and bind group associated to a texture that has been allocated by egui.
    ///
    /// This could be used by custom paint hooks to render images that have been added through with
    /// [`egui_extras::RetainedImage`](https://docs.rs/egui_extras/latest/egui_extras/image/struct.RetainedImage.html)
    /// or [`epaint::Context::load_texture`](https://docs.rs/egui/latest/egui/struct.Context.html#method.load_texture).
    pub fn texture(&self, id: &epaint::TextureId) -> Option<&ResourceHandle<Texture>> {
        let index = self.textures_to_index.get(id)?;
        self.textures.get(*index)
    }

    pub fn update_buffers(&mut self, ctx: &RenderContext, paint_jobs: &[epaint::ClippedPrimitive]) {
        // Determine how many vertices & indices need to be rendered, and gather prepare callbacks
        let (vertex_count, index_count) = {
            paint_jobs.iter().fold((0, 0), |acc, clipped_primitive| {
                match &clipped_primitive.primitive {
                    Primitive::Mesh(mesh) => {
                        (acc.0 + mesh.vertices.len(), acc.1 + mesh.indices.len())
                    }
                    Primitive::Callback(_) => {
                        unimplemented!();
                        // if let Some(c) = callback.callback.downcast_ref::<Callback>() {
                        //     callbacks.push(c.0.as_ref());
                        // } else {
                        //     log::warn!("Unknown paint callback: expected `egui_wgpu::Callback`");
                        // };
                        // acc
                    }
                }
            })
        };

        if index_count > 0 {
            self.index_buffer.slices.clear();
            let required_index_buffer_size = (std::mem::size_of::<u32>() * index_count) as u64;
            if self.index_buffer.capacity < required_index_buffer_size {
                // Resize index buffer if needed.
                self.index_buffer.capacity =
                    (self.index_buffer.capacity * 2).at_least(required_index_buffer_size);
                self.index_buffer.buffer = create_index_buffer(ctx, self.index_buffer.capacity);
            }

            // let mut index_buffer_staging = queue
            //     .write_buffer_with(
            //         &self.index_buffer.buffer,
            //         0,
            //         NonZeroU64::new(required_index_buffer_size).unwrap(),
            //     )
            //     .expect("Failed to create staging buffer for index data");
            let mut buffer = ctx.buffer_manager.get(&self.index_buffer.buffer).unwrap();
            let mut index_offset = 0;
            for epaint::ClippedPrimitive { primitive, .. } in paint_jobs.iter() {
                match primitive {
                    Primitive::Mesh(mesh) => {
                        let size = mesh.indices.len() * std::mem::size_of::<u32>();
                        let slice = index_offset..(size + index_offset);

                        buffer.copy_from_slice(&mesh.indices, index_offset);
                        self.index_buffer.slices.push(slice);
                        index_offset += size;
                    }
                    Primitive::Callback(_) => {}
                }
            }
        }

        if vertex_count > 0 {
            self.vertex_buffer.slices.clear();
            let required_vertex_buffer_size = (std::mem::size_of::<Vertex>() * vertex_count) as u64;
            if self.vertex_buffer.capacity < required_vertex_buffer_size {
                // Resize vertex buffer if needed.
                self.vertex_buffer.capacity =
                    (self.vertex_buffer.capacity * 2).at_least(required_vertex_buffer_size);
                self.vertex_buffer.buffer = create_vertex_buffer(ctx, self.vertex_buffer.capacity);
            }

            let mut vertex_offset = 0;
            let mut buffer = ctx.buffer_manager.get(&self.vertex_buffer.buffer).unwrap();
            for epaint::ClippedPrimitive { primitive, .. } in paint_jobs.iter() {
                match primitive {
                    Primitive::Mesh(mesh) => {
                        let size = mesh.vertices.len() * std::mem::size_of::<Vertex>();
                        let slice = vertex_offset..(size + vertex_offset);
                        buffer.copy_from_slice(&mesh.vertices, vertex_offset);
                        self.vertex_buffer.slices.push(slice);
                        vertex_offset += size;
                    }
                    Primitive::Callback(_) => {}
                }
            }
        }
    }

    pub fn render(
        &self,
        ctx: &RenderContext,
        paint_jobs: &[epaint::ClippedPrimitive],
        screen_descriptor: &ScreenDescriptor,
        command_buffer: vk::CommandBuffer,
    ) {
        unsafe {
            let mut pipeline = ctx.graphics_pipelines.get_mut(&self.pipeline).unwrap();
            let pixels_per_point = screen_descriptor.pixels_per_point;
            let size_in_pixels = screen_descriptor.size_in_pixels;

            // Whether or not we need to reset the render pass because a paint callback has just
            // run.

            let mut index_buffer_slices = self.index_buffer.slices.iter();
            let mut vertex_buffer_slices = self.vertex_buffer.slices.iter();

            ctx.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                pipeline.pipeline,
            );
            pipeline.bind_descriptor_sets(ctx, command_buffer);
            ctx.device.cmd_set_viewport(
                command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: size_in_pixels[0] as f32,
                    height: size_in_pixels[1] as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            for epaint::ClippedPrimitive {
                clip_rect,
                primitive,
            } in paint_jobs
            {
                let rect = ScissorRect::new(clip_rect, pixels_per_point, size_in_pixels);

                if rect.width == 0 || rect.height == 0 {
                    // Skip rendering zero-sized clip areas.
                    if let Primitive::Mesh(_) = primitive {
                        // If this is a mesh, we need to advance the index and vertex buffer iterators:
                        index_buffer_slices.next().unwrap();
                        vertex_buffer_slices.next().unwrap();
                    }
                    continue;
                }

                ctx.device.cmd_set_scissor(
                    command_buffer,
                    0,
                    &[vk::Rect2D {
                        extent: vk::Extent2D {
                            width: rect.width,
                            height: rect.height,
                        },
                        offset: vk::Offset2D {
                            x: rect.x as i32,
                            y: rect.y as i32,
                        },
                    }],
                );

                match primitive {
                    Primitive::Mesh(mesh) => {
                        let index_buffer_slice = index_buffer_slices.next().unwrap();
                        let vertex_buffer_slice = vertex_buffer_slices.next().unwrap();
                        if let Some(texture_index) = self.textures_to_index.get(&mesh.texture_id) {
                            let index_buffer =
                                ctx.buffer_manager.get(&self.index_buffer.buffer).unwrap();
                            let vertex_buffer =
                                ctx.buffer_manager.get(&self.vertex_buffer.buffer).unwrap();

                            ctx.device.cmd_push_constants(
                                command_buffer,
                                pipeline.layout,
                                vk::ShaderStageFlags::ALL_GRAPHICS,
                                0,
                                bytemuck::bytes_of(&PushConstants {
                                    texture_index: *texture_index as u32,
                                    screen_size: screen_descriptor.screen_size_in_points(),
                                    _pad: Default::default(),
                                }),
                            );

                            ctx.device.cmd_bind_index_buffer(
                                command_buffer,
                                index_buffer.buffer,
                                index_buffer_slice.start as _,
                                vk::IndexType::UINT32,
                            );

                            ctx.device.cmd_bind_vertex_buffers(
                                command_buffer,
                                0,
                                &[vertex_buffer.buffer],
                                &[vertex_buffer_slice.start as _],
                            );

                            ctx.device.cmd_draw_indexed(
                                command_buffer,
                                mesh.indices.len() as u32,
                                1,
                                0,
                                0,
                                0,
                            );
                        } else {
                            log::warn!("Missing texture: {:?}", mesh.texture_id);
                        }
                    }
                    Primitive::Callback(_) => {
                        unimplemented!();
                    }
                }
            }

            ctx.device.cmd_set_scissor(
                command_buffer,
                0,
                &[vk::Rect2D {
                    extent: vk::Extent2D {
                        width: size_in_pixels[0],
                        height: size_in_pixels[1],
                    },
                    offset: vk::Offset2D { x: 0, y: 0 },
                }],
            );
        }
    }
}

/// A Rect in physical pixel space, used for setting clipping rectangles.
struct ScissorRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ScissorRect {
    fn new(clip_rect: &epaint::Rect, pixels_per_point: f32, target_size: [u32; 2]) -> Self {
        // Transform clip rect to physical pixels:
        let clip_min_x = pixels_per_point * clip_rect.min.x;
        let clip_min_y = pixels_per_point * clip_rect.min.y;
        let clip_max_x = pixels_per_point * clip_rect.max.x;
        let clip_max_y = pixels_per_point * clip_rect.max.y;

        // Round to integer:
        let clip_min_x = clip_min_x.round() as u32;
        let clip_min_y = clip_min_y.round() as u32;
        let clip_max_x = clip_max_x.round() as u32;
        let clip_max_y = clip_max_y.round() as u32;

        // Clamp:
        let clip_min_x = clip_min_x.clamp(0, target_size[0]);
        let clip_min_y = clip_min_y.clamp(0, target_size[1]);
        let clip_max_x = clip_max_x.clamp(clip_min_x, target_size[0]);
        let clip_max_y = clip_max_y.clamp(clip_min_y, target_size[1]);

        Self {
            x: clip_min_x,
            y: clip_min_y,
            width: clip_max_x - clip_min_x,
            height: clip_max_y - clip_min_y,
        }
    }
}
