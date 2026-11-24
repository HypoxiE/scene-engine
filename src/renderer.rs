use crate::{project::{AssetType, Project, TextAlign}, scene::{Scene, SceneGeometry, Vertex}};
use anyhow::{Context, Result, bail};
use bytemuck::{Zeroable, cast_slice};
use image::GenericImageView;
use std::{collections::HashMap, mem::size_of, path::Path, sync::Arc};
use fontdb::{Database as FontDatabase, Family, Query, Stretch, Style, Weight};
use fontdue::{Font, FontSettings};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

struct GpuTexture {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

#[derive(Clone, Debug, PartialEq)]
struct TextCacheKey {
    text: String,
    font_family: String,
    font_size_bits: u32,
    size_bits: [u32; 2],
    color_bits: [u32; 4],
    alignment: TextAlign,
}

impl TextCacheKey {
    fn from_node(node: &crate::scene::SceneNode) -> Self {
        Self {
            text: node.text.clone().unwrap_or_default(),
            font_family: node.font_family.clone(),
            font_size_bits: node.font_size.to_bits(),
            size_bits: [node.size[0].to_bits(), node.size[1].to_bits()],
            color_bits: node.color.map(f32::to_bits),
            alignment: node.text_align,
        }
    }
}

pub struct GpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    textures: HashMap<String, GpuTexture>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    font_database: FontDatabase,
    fonts: HashMap<String, Font>,
    text_cache: HashMap<String, TextCacheKey>,
    vertex_buffer: wgpu::Buffer,
    geometry: SceneGeometry,
    clear_color: [f64; 4],
}

impl GpuRenderer {
    pub async fn new(
        window: Arc<Window>,
        display_handle: winit::event_loop::OwnedDisplayHandle,
        project: &Project,
        scene: &Scene,
    ) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display_handle)),
        );
        let surface = instance
            .create_surface(window)
            .context("Не удалось создать поверхность wgpu для окна")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .context("Не найден совместимый графический адаптер")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Scene Engine Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("Не удалось создать логическое GPU-устройство")?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("Графический адаптер не поддерживает вывод в это окно")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite Texture Bind Group Layout"),
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sprite Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite Pipeline Layout"),
            bind_group_layouts: &[Some(&texture_layout)],
            immediate_size: 0,
        });

        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let mut textures = HashMap::new();
        for asset in &project.manifest.assets {
            match asset.kind {
                AssetType::Image => {
                    let path = project.root.join(&asset.path);
                    let texture = load_texture(
                        &device,
                        &queue,
                        &texture_layout,
                        &sampler,
                        &path,
                        asset.columns,
                        asset.rows,
                    )
                    .with_context(|| format!("Не удалось загрузить текстуру '{}'", asset.id))?;
                    textures.insert(asset.id.clone(), texture);
                }
            }
        }

        // Text is rasterized with a normal installed system font. On NixOS the
        // devShell also exposes DejaVu Sans through SCENE_ENGINE_FONT_DIR.
        let mut font_database = FontDatabase::new();
        font_database.load_system_fonts();
        if let Some(font_dir) = std::env::var_os("SCENE_ENGINE_FONT_DIR") {
            font_database.load_fonts_dir(Path::new(&font_dir));
        }
        let mut fonts = HashMap::new();
        let mut text_cache = HashMap::new();
        for node in scene.nodes.iter().filter(|node| matches!(node.kind, crate::project::NodeType::Text)) {
            let key = TextCacheKey::from_node(node);
            let font = get_font(&font_database, &mut fonts, &node.font_family)?;
            let (pixels, text_width, text_height) = rasterize_text(font, &key.text, node.font_size, node.size, node.color, node.text_align)?;
            let texture = create_rgba_texture(
                &device,
                &queue,
                &texture_layout,
                &sampler,
                &pixels,
                text_width,
                text_height,
                "Text Node Texture",
            );
            textures.insert(format!("__text:{}", node.id), texture);
            text_cache.insert(node.id.clone(), key);
        }

        let geometry = SceneGeometry::build(scene, config.width, config.height);
        let initial_vertices = if geometry.vertices.is_empty() {
            cast_slice(&[Vertex::zeroed()]).to_vec()
        } else {
            cast_slice(&geometry.vertices).to_vec()
        };
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scene Vertex Buffer"),
            contents: &initial_vertices,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            textures,
            texture_layout,
            sampler,
            font_database,
            fonts,
            text_cache,
            vertex_buffer,
            geometry,
            clear_color: project.manifest.clear_color,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>, scene: &Scene) {
        if size.width == 0 || size.height == 0 {
            return;
        }

        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.rebuild_geometry(scene);
    }

    fn rebuild_geometry(&mut self, scene: &Scene) {
        self.geometry = SceneGeometry::build(scene, self.config.width, self.config.height);
        // The first version does not create/delete nodes at runtime, so the vertex
        // count remains fixed. Invisible nodes keep their slots with zero opacity.
        if !self.geometry.vertices.is_empty() {
            self.queue.write_buffer(&self.vertex_buffer, 0, cast_slice(&self.geometry.vertices));
        }
    }

    fn sync_textures(&mut self, scene: &Scene) -> Result<()> {
        for node in scene.nodes.iter().filter(|node| matches!(node.kind, crate::project::NodeType::Text)) {
            let key = TextCacheKey::from_node(node);
            if self.text_cache.get(&node.id) == Some(&key) {
                continue;
            }

            let font = get_font(&self.font_database, &mut self.fonts, &node.font_family)?;
            let (pixels, width, height) = rasterize_text(font, &key.text, node.font_size, node.size, node.color, node.text_align)
                .with_context(|| format!("Не удалось растеризовать текстовый объект '{}'", node.id))?;
            let texture = create_rgba_texture(
                &self.device,
                &self.queue,
                &self.texture_layout,
                &self.sampler,
                &pixels,
                width,
                height,
                "Dynamic Text Node Texture",
            );
            self.textures.insert(format!("__text:{}", node.id), texture);
            self.text_cache.insert(node.id.clone(), key);
        }
        Ok(())
    }

    pub fn render(&mut self, scene: &Scene) -> Result<bool> {
        self.sync_textures(scene)?;
        self.rebuild_geometry(scene);
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.draw_frame(frame);
                self.surface.configure(&self.device, &self.config);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                bail!("wgpu сообщил об ошибке проверки поверхности")
            }
        };

        self.draw_frame(frame);
        Ok(false)
    }

    fn draw_frame(&mut self, frame: wgpu::SurfaceTexture) {
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Scene Render Encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.clear_color[0],
                            g: self.clear_color[1],
                            b: self.clear_color[2],
                            a: self.clear_color[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if !self.geometry.vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

                for command in &self.geometry.commands {
                    if let Some(texture) = self.textures.get(&command.asset_id) {
                        pass.set_bind_group(0, &texture.bind_group, &[]);
                        pass.draw(command.vertices.clone(), 0..1);
                    }
                }
            }
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }
}

fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    path: &Path,
    columns: u32,
    rows: u32,
) -> Result<GpuTexture> {
    let image = image::open(path)
        .with_context(|| format!("Не удалось декодировать изображение {}", path.display()))?;
    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();
    anyhow::ensure!(width % columns == 0, "Ширина текстуры должна делиться на columns ({columns})");
    anyhow::ensure!(height % rows == 0, "Высота текстуры должна делиться на rows ({rows})");

    Ok(create_rgba_texture(
        device,
        queue,
        texture_layout,
        sampler,
        rgba.as_raw(),
        width,
        height,
        "Project Image Texture",
    ))
}



fn create_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    pixels: &[u8],
    width: u32,
    height: u32,
    label: &str,
) -> GpuTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Sprite/Text Texture Bind Group"),
        layout: texture_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });

    GpuTexture { _texture: texture, bind_group }
}

fn get_font<'a>(
    database: &FontDatabase,
    cache: &'a mut HashMap<String, Font>,
    family_name: &str,
) -> Result<&'a Font> {
    if !cache.contains_key(family_name) {
        let families = if family_name.eq_ignore_ascii_case("sans-serif") {
            vec![Family::SansSerif]
        } else if family_name.eq_ignore_ascii_case("serif") {
            vec![Family::Serif]
        } else if family_name.eq_ignore_ascii_case("monospace") {
            vec![Family::Monospace]
        } else {
            vec![Family::Name(family_name), Family::SansSerif]
        };
        let id = database.query(&Query {
            families: &families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        }).context(format!("Не найден системный шрифт '{family_name}'"))?;
        let (data, collection_index) = database
            .with_face_data(id, |data, index| (data.to_vec(), index))
            .context(format!("Не удалось прочитать системный шрифт '{family_name}'"))?;
        let font = Font::from_bytes(
            data,
            FontSettings { collection_index, ..FontSettings::default() },
        ).map_err(|error| anyhow::anyhow!("Не удалось разобрать шрифт '{family_name}': {error:?}"))?;
        cache.insert(family_name.to_owned(), font);
    }
    Ok(cache.get(family_name).expect("font was just inserted"))
}

fn rasterize_text(
    font: &Font,
    text: &str,
    font_size: f32,
    box_size: [f32; 2],
    color: [f32; 4],
    alignment: TextAlign,
) -> Result<(Vec<u8>, u32, u32)> {
    anyhow::ensure!(text.chars().count() <= 4096, "Текстовый объект слишком длинный");

    // `size` describes the text layout box, not a scale factor for the glyphs.
    // This preserves the font's proportions even when the string changes at runtime.
    let width = box_size[0].ceil().max(1.0) as u32;
    let height = box_size[1].ceil().max(1.0) as u32;
    anyhow::ensure!(width <= 8192 && height <= 1024, "Текстовый объект слишком велик");

    let px = font_size.max(1.0);
    let (ascent, line_height) = match font.horizontal_line_metrics(px) {
        Some(metrics) => (metrics.ascent, metrics.new_line_size),
        None => (px * 0.8, px * 1.25),
    };
    let baseline = (((height as f32 - line_height).max(0.0) * 0.5) + ascent).round() as i32;

    let mut glyphs = Vec::new();
    let mut pen_x = 0.0_f32;
    let mut previous = None;

    for character in text.chars() {
        if character == '\n' {
            // Text nodes are intentionally single-line in the first text renderer.
            continue;
        }
        if let Some(previous_character) = previous {
            pen_x += font.horizontal_kern(previous_character, character, px).unwrap_or(0.0);
        }
        let (metrics, bitmap) = font.rasterize(character, px);
        let glyph_x = pen_x.floor() as i32 + metrics.xmin;
        let glyph_y = baseline - metrics.ymin - metrics.height as i32;
        glyphs.push((glyph_x, glyph_y, metrics.width, metrics.height, bitmap));
        pen_x += metrics.advance_width;
        previous = Some(character);
    }

    let text_advance = pen_x.ceil() as i32;
    let horizontal_offset = match alignment {
        TextAlign::Left => 0,
        TextAlign::Center => (width as i32 - text_advance) / 2,
        TextAlign::Right => width as i32 - text_advance,
    };

    let mut rgba = vec![0_u8; (width * height * 4) as usize];
    let rgb = [
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ];

    for (glyph_x, glyph_y, glyph_width, glyph_height, bitmap) in glyphs {
        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let target_x = glyph_x + horizontal_offset + x as i32;
                let target_y = glyph_y + y as i32;
                if target_x < 0 || target_y < 0 || target_x >= width as i32 || target_y >= height as i32 {
                    continue;
                }

                let coverage = bitmap[y * glyph_width + x] as f32 / 255.0;
                let alpha = (coverage * color[3].clamp(0.0, 1.0) * 255.0).round() as u8;
                let offset = ((target_y as u32 * width + target_x as u32) * 4) as usize;
                rgba[offset] = rgb[0];
                rgba[offset + 1] = rgb[1];
                rgba[offset + 2] = rgb[2];
                rgba[offset + 3] = alpha;
            }
        }
    }

    Ok((rgba, width, height))
}
