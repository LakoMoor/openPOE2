//! `viewer <Bundles2> <model .smd/.fmt> [--screenshot out.png [yaw-degrees]] [--size WxH]`
//!
//! Interactive wgpu viewer: drag to orbit, wheel to zoom, Space toggles
//! auto-rotation, Esc quits. With `--screenshot` it renders one frame offscreen
//! (no window) and saves it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use image::{imageops, RgbaImage};
use poe2_bundles::{index::Index, mesh, model};
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const SHADER: &str = r#"
struct Globals { view_proj: mat4x4<f32>, light: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
struct Params { flags: vec4<f32> };
@group(1) @binding(0) var<uniform> p: Params;
@group(1) @binding(1) var t: texture_2d<f32>;
@group(1) @binding(2) var s: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = g.view_proj * vec4<f32>(pos, 1.0);
    o.n = n;
    o.uv = uv;
    return o;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var base = vec3<f32>(0.62, 0.45, 0.30);
    let c = textureSample(t, s, in.uv);
    if (p.flags.y > 0.5) {
        if (p.flags.x > 0.5 && c.a < 0.5) { discard; }
        base = c.rgb;
    }
    let n = normalize(in.n);
    let key = abs(dot(n, normalize(g.light.xyz)));
    let fill = abs(dot(n, normalize(vec3<f32>(-0.5, -0.2, 0.8)))) * 0.25;
    return vec4<f32>(base * (0.22 + 0.78 * key + fill), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
}

struct Draw {
    first_index: u32,
    index_count: u32,
    bind: wgpu::BindGroup,
}

struct Scene {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draws: Vec<Draw>,
    /// Bounding radius of the centred model.
    radius: f32,
}

/// A character pose driven by an animation clip, skinned on the CPU every frame.
struct Clip {
    tracks: Vec<poe2_bundles::skeleton::Track>,
    fps: f32,
    /// Clip length in frames.
    length: f32,
}

struct Anim {
    skeleton: Arc<poe2_bundles::skeleton::Skeleton>,
    clips: Vec<Clip>,
    current: usize,
    /// Index of the character's first vertex in the scene's vertex buffer.
    base: usize,
    rest_pos: Vec<[f32; 3]>,
    rest_nor: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u8; 4]>,
    weights: Vec<[u8; 4]>,
    center: [f32; 3],
}

struct Renderer {
    anim: Option<Anim>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    shape_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    scene: Option<Scene>,
}

/// Build the mip chain of `img` (level 0 first).
fn mip_chain(img: &RgbaImage) -> Vec<RgbaImage> {
    let mut levels = vec![img.clone()];
    while levels.last().is_some_and(|l| l.width() > 1 || l.height() > 1) {
        let l = levels.last().unwrap();
        let (w, h) = ((l.width() / 2).max(1), (l.height() / 2).max(1));
        levels.push(imageops::resize(l, w, h, imageops::FilterType::Triangle));
    }
    levels
}

impl Renderer {
    fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: wgpu::TextureFormat,
    ) -> Self {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let shape_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shape layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bind"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&shape_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(target.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self { anim: None, device, queue, pipeline, globals, globals_bind, shape_layout, sampler, scene: None }
    }

    /// Replace the displayed model.
    fn set_model(&mut self, mesh: &mesh::Mesh, textures: &[Option<model::ShapeTex>]) {
        let (device, queue) = (&self.device, &self.queue);
        let (shape_layout, sampler) = (&self.shape_layout, &self.sampler);
        // Centre the model and turn its Z-down axes into Y-up.
        let b = mesh.bbox;
        let c = [(b[0] + b[1]) / 2.0, (b[2] + b[3]) / 2.0, (b[4] + b[5]) / 2.0];
        let to_view = |v: [f32; 3]| [v[0], -v[2], v[1]];
        let mut radius = 0.0f32;
        let vertices: Vec<Vertex> = mesh
            .positions
            .iter()
            .zip(&mesh.normals)
            .zip(&mesh.uvs)
            .map(|((p, n), uv)| {
                let pos = to_view([p[0] - c[0], p[1] - c[1], p[2] - c[2]]);
                radius = radius.max(Vec3::from(pos).length());
                Vertex { pos, normal: to_view(*n), uv: *uv }
            })
            .collect();
        let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let white = RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));

        let draws = mesh
            .shapes
            .iter()
            .zip(textures)
            .map(|(shape, tex)| {
                let (img, alpha_test) = match tex {
                    Some(t) => (&t.img, t.alpha_test),
                    None => (&white, false),
                };
                let levels = mip_chain(img);
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some("shape texture"),
                        size: wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
                        mip_level_count: levels.len() as u32,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &levels.iter().flat_map(|l| l.as_raw().iter().copied()).collect::<Vec<u8>>(),
                );
                let flags: [f32; 4] = [alpha_test as u32 as f32, tex.is_some() as u32 as f32, 0.0, 0.0];
                let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("shape params"),
                    contents: bytemuck::cast_slice(&flags),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shape bind"),
                    layout: shape_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                            ),
                        },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                    ],
                });
                let hidden = tex.as_ref().is_some_and(|t| t.invisible);
                Draw { first_index: shape.first_index, index_count: if hidden { 0 } else { shape.index_count }, bind }
            })
            .collect();

        self.anim = None;
        self.scene = Some(Scene { vertices: vertex_buf, indices: index_buf, draws, radius: radius.max(1e-3) });
    }

    /// Drive the character stored at vertex `base` of the current scene with
    /// animation clips. `center` is the centre used when the scene was uploaded.
    fn set_animation(&mut self, mesh: &mesh::Mesh, clips: Vec<Clip>, base: usize, center: [f32; 3]) {
        let Some(skeleton) = mesh.rig.clone() else { return };
        if mesh.joints.len() != mesh.positions.len() || clips.is_empty() {
            return;
        }
        self.anim = Some(Anim {
            skeleton,
            clips,
            current: 0,
            base,
            rest_pos: mesh.positions.clone(),
            rest_nor: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            joints: mesh.joints.clone(),
            weights: mesh.weights.clone(),
            center,
        });
    }

    /// Pose the character at `seconds` into the (looping) clip, then move it by `place`.
    fn animate(&self, seconds: f32, place: Mat4) {
        let (Some(a), Some(scene)) = (&self.anim, &self.scene) else { return };
        let clip = &a.clips[a.current];
        let frame = (seconds * clip.fps) % clip.length;
        let skin = a.skeleton.skin_matrices(&a.skeleton.pose(&clip.tracks, frame));
        let c = a.center;
        let to_view = |v: Vec3| [v.x, -v.z, v.y];
        let vertices: Vec<Vertex> = (0..a.rest_pos.len())
            .map(|i| {
                let mut m = Mat4::ZERO;
                for k in 0..4 {
                    let w = a.weights[i][k] as f32 / 255.0;
                    if w > 0.0 {
                        m += skin.get(a.joints[i][k] as usize).copied().unwrap_or(Mat4::IDENTITY) * w;
                    }
                }
                let m = place * m;
                let p = m.transform_point3(Vec3::from(a.rest_pos[i])) - Vec3::from(c);
                let n = m.transform_vector3(Vec3::from(a.rest_nor[i])).normalize_or_zero();
                Vertex { pos: to_view(p), normal: to_view(n), uv: a.uvs[i] }
            })
            .collect();
        let offset = (a.base * std::mem::size_of::<Vertex>()) as u64;
        self.queue.write_buffer(&scene.vertices, offset, bytemuck::cast_slice(&vertices));
    }

    fn render(&self, color: &wgpu::TextureView, depth: &wgpu::TextureView, size: (u32, u32), cam: &Camera) {
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let r = self.scene.as_ref().map_or(1.0, |s| s.radius);
        let proj = perspective(40f32.to_radians(), aspect, r * 0.05, r * 40.0);
        let view = look_at(cam.eye(r) + cam.target, cam.target, Vec3::Y);
        let g = Globals { view_proj: (proj * view).to_cols_array_2d(), light: [0.4, 0.7, 0.6, 0.0] };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));

        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.004, g: 0.003, b: 0.002, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            if let Some(scene) = &self.scene {
                pass.set_vertex_buffer(0, scene.vertices.slice(..));
                pass.set_index_buffer(scene.indices.slice(..), wgpu::IndexFormat::Uint32);
                for d in &scene.draws {
                    pass.set_bind_group(1, &d.bind, &[]);
                    pass.draw_indexed(d.first_index..d.first_index + d.index_count, 0, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
    }

    fn depth_view(&self, size: (u32, u32)) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
}

/// Right-handed perspective with a 0..1 depth range (wgpu clip space).
fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    let d = near - far;
    Mat4::from_cols_array(&[
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, far / d, -1.0,
        0.0, 0.0, near * far / d, 0.0,
    ])
}

fn look_at(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols_array(&[
        s.x, u.x, -f.x, 0.0,
        s.y, u.y, -f.y, 0.0,
        s.z, u.z, -f.z, 0.0,
        -s.dot(eye), -u.dot(eye), f.dot(eye), 1.0,
    ])
}

#[derive(Clone, Copy)]
struct Camera {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// Point the camera orbits, in view space.
    target: Vec3,
}

impl Camera {
    fn eye(&self, radius: f32) -> Vec3 {
        let d = radius * 3.4 * self.zoom;
        Vec3::new(
            d * self.pitch.cos() * self.yaw.sin(),
            d * self.pitch.sin(),
            d * self.pitch.cos() * self.yaw.cos(),
        )
    }
}

async fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let mut limits = wgpu::Limits::default();
    limits.max_texture_dimension_2d = adapter.limits().max_texture_dimension_2d.min(16384);
    Ok(adapter
        .request_device(&wgpu::DeviceDescriptor { label: Some("device"), required_limits: limits, ..Default::default() })
        .await?)
}

/// Load an animation clip from `set[:name]` (default: the first clip with "idle" in its name).
/// An exact name wins over a partial match.
fn load_clip(idx: &Index, dir: &std::path::Path, spec: &str) -> Option<Clip> {
    use poe2_bundles::skeleton::Skeleton;
    let (set, name) = spec.split_once(':').unwrap_or((spec, "idle"));
    let bytes = idx.read_file(dir, &format!("art/models/charactersfour/animations/{set}/onerig.ast")).ok()?;
    let sk = Skeleton::parse(&bytes).ok()?;
    let name = name.to_ascii_lowercase();
    let clip = sk
        .animations
        .iter()
        .find(|a| a.name.to_ascii_lowercase() == name)
        .or_else(|| sk.animations.iter().find(|a| a.name.to_ascii_lowercase().contains(&name)))?;
    println!("animation {set}/{} ({} fps)", clip.name, clip.framerate);
    let tracks = sk.tracks(clip).ok()?;
    let length = tracks.iter().flat_map(|t| t.rotations.iter().map(|k| k[0])).fold(1.0f32, f32::max);
    Some(Clip { tracks, fps: clip.framerate as f32, length })
}

fn mesh_center(mesh: &mesh::Mesh) -> [f32; 3] {
    let b = mesh.bbox;
    [(b[0] + b[1]) / 2.0, (b[2] + b[3]) / 2.0, (b[4] + b[5]) / 2.0]
}

fn offscreen_renderer() -> Result<Renderer> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .context("no graphics adapter")?;
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(request_device(&adapter))?;
    Ok(Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb))
}

fn screenshot(
    mesh: &mesh::Mesh,
    textures: &[Option<model::ShapeTex>],
    clip: Option<Clip>,
    time: f32,
    size: (u32, u32),
    yaw: f32,
    out: &str,
) -> Result<()> {
    let mut renderer = offscreen_renderer()?;
    renderer.set_model(mesh, textures);
    if let Some(clip) = clip {
        renderer.set_animation(mesh, vec![clip], 0, mesh_center(mesh));
        renderer.animate(time, Mat4::IDENTITY);
    }
    capture(&renderer, size, &Camera { yaw, pitch: 0.25, zoom: 1.0, target: Vec3::ZERO }, out)
}

/// Render the character standing (or, with `run`, running for `seconds`) in a world.
fn screenshot_world(
    idx: &Index,
    dir: &std::path::Path,
    world: &str,
    character: &str,
    seconds: f32,
    run: bool,
    size: (u32, u32),
    yaw: f32,
    out: &str,
) -> Result<()> {
    let mut renderer = offscreen_renderer()?;
    let mut play = show_play(idx, dir, world, character, "baseunarmed", &mut renderer, None).context("loading the world")?;
    let mut cam = Camera { yaw, pitch: 0.6, zoom: 1.0, target: Vec3::ZERO };
    // Walk forward in small steps so the camera and pose settle like in the window.
    play.keys[0] = run;
    if std::env::var_os("POE2_ATTACK").is_some() {
        play.attack = Some(0.0);
    }
    let steps = (seconds * 60.0).max(1.0) as u32;
    for i in 0..steps {
        play.step(1.0 / 60.0, &mut cam, i as f32 / 60.0, &mut renderer);
    }
    capture(&renderer, size, &cam, out)
}

fn capture(renderer: &Renderer, size: (u32, u32), cam: &Camera, out: &str) -> Result<()> {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let color = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = renderer.depth_view(size);
    renderer.render(&color.create_view(&Default::default()), &depth, size, cam);

    let row = (size.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buf = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (row * size.1) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = renderer.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(size.1) },
        },
        wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
    );
    renderer.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    renderer.device.poll(wgpu::PollType::wait_indefinitely())?;
    let data = buf.slice(..).get_mapped_range().expect("mapped range");
    let mut img = RgbaImage::new(size.0, size.1);
    for y in 0..size.1 {
        let src = &data[(y * row) as usize..(y * row + size.0 * 4) as usize];
        for x in 0..size.0 {
            let p = &src[(x * 4) as usize..(x * 4 + 4) as usize];
            img.put_pixel(x, y, image::Rgba([p[0], p[1], p[2], 255]));
        }
    }
    img.save(out)?;
    println!("-> {out}");
    Ok(())
}

struct Window3d {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    renderer: Renderer,
}

struct App {
    idx: Index,
    dir: PathBuf,
    list: Vec<String>,
    pos: usize,
    state: Option<Window3d>,
    cam: Camera,
    auto_rotate: bool,
    dragging: bool,
    last_cursor: Option<(f64, f64)>,
    last_frame: Instant,
    anim_spec: Option<String>,
    clock: f32,
    world: Option<String>,
    play: Option<Play>,
}

/// Load `list[pos]` into the renderer and describe it in the window title.
fn show(idx: &Index, dir: &std::path::Path, list: &[String], pos: usize, anim: Option<&str>, renderer: &mut Renderer, window: &Window) {
    let path = &list[pos];
    let result = model::load_mesh(idx, dir, path);
    match result {
        Ok(mesh) => {
            let tex = model::load_shape_textures(idx, dir, path, &mesh);
            window.set_title(&format!(
                "openPOE2 [{}/{}] {path} - {} triangles, {}/{} textured",
                pos + 1,
                list.len(),
                mesh.indices.len() / 3,
                tex.iter().flatten().count(),
                tex.len()
            ));
            renderer.set_model(&mesh, &tex);
            if let (Some(spec), Some(_)) = (anim, &mesh.rig) {
                if let Some(clip) = load_clip(idx, dir, spec) {
                    renderer.set_animation(&mesh, vec![clip], 0, mesh_center(&mesh));
                }
            }
        }
        Err(e) => {
            println!("{path}: {e:#}");
            window.set_title(&format!("openPOE2 [{}/{}] {path} - cannot load: {e}", pos + 1, list.len()));
            renderer.scene = None;
        }
    }
}

/// A character walking around a loaded world with WASD.
struct Play {
    keys: [bool; 4],
    /// Position on the ground plane (model space).
    pos: glam::Vec2,
    heading: f32,
    center: [f32; 3],
    running: bool,
    /// Seconds into the attack clip while one is playing.
    attack: Option<f32>,
    /// Seconds into the current clip, and which clip that was.
    clip_time: f32,
    clip: usize,
}

const RUN_SPEED: f32 = 350.0;

impl Play {
    fn step(&mut self, dt: f32, cam: &mut Camera, clock: f32, renderer: &mut Renderer) {
        let [w, a, s, d] = self.keys;
        // Attacks root the character until the clip ends.
        let attack_len = renderer.anim.as_ref().and_then(|an| an.clips.get(2)).map(|c| c.length / c.fps);
        if let Some(t) = self.attack.as_mut() {
            *t += dt;
            if attack_len.is_none_or(|l| *t >= l) {
                self.attack = None;
            }
        }
        let [w, a, s, d] = if self.attack.is_some() { [false; 4] } else { [w, a, s, d] };
        // Screen-relative axes on the ground plane (model x and y map to view x and z).
        let fwd = glam::Vec2::new(-cam.yaw.sin(), -cam.yaw.cos());
        let right = glam::Vec2::new(cam.yaw.cos(), -cam.yaw.sin());
        let dir = fwd * (w as i32 - s as i32) as f32 + right * (d as i32 - a as i32) as f32;
        self.running = dir.length_squared() > 0.0;
        if self.running {
            let dir = dir.normalize();
            self.pos += dir * RUN_SPEED * dt;
            // The model faces -Y; turn smoothly towards the movement direction.
            let want = dir.x.atan2(-dir.y);
            let tau = std::f32::consts::TAU;
            let mut diff = (want - self.heading + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
            diff *= (dt * 12.0).min(1.0);
            self.heading += diff;
        }
        if let Some(anim) = renderer.anim.as_mut() {
            let want = if self.attack.is_some() { 2 } else { self.running as usize };
            if anim.current != want && want < anim.clips.len() {
                anim.current = want;
            }
            if anim.current != self.clip {
                self.clip = anim.current;
                self.clip_time = 0.0;
            }
        }
        self.clip_time += dt;
        let place = Mat4::from_translation(Vec3::new(self.pos.x, self.pos.y, 0.0)) * Mat4::from_rotation_z(self.heading);
        let _ = clock;
        renderer.animate(self.clip_time, place);
        let c = self.center;
        cam.target = Vec3::new(self.pos.x - c[0], c[2] + 90.0, self.pos.y - c[1]);
    }
}

/// Load the world and the character into one scene; returns the play state.
fn show_play(
    idx: &Index,
    dir: &std::path::Path,
    world: &str,
    character: &str,
    set: &str,
    renderer: &mut Renderer,
    window: Option<&Window>,
) -> Option<Play> {
    let load = |path: &str| -> Option<(mesh::Mesh, Vec<Option<model::ShapeTex>>)> {
        match model::load_mesh(idx, dir, path) {
            Ok(m) => {
                let t = model::load_shape_textures(idx, dir, path, &m);
                Some((m, t))
            }
            Err(e) => {
                println!("{path}: {e:#}");
                None
            }
        }
    };
    let (world_mesh, mut textures) = load(world)?;
    let (char_mesh, char_tex) = load(character)?;
    let wb = world_mesh.bbox;
    let start = glam::Vec2::new((wb[0] + wb[1]) / 2.0, (wb[2] + wb[3]) / 2.0);
    let base = world_mesh.positions.len();

    let mut all = world_mesh;
    all.joints.clear();
    all.weights.clear();
    all.positions.extend_from_slice(&char_mesh.positions);
    all.normals.extend_from_slice(&char_mesh.normals);
    all.uvs.extend_from_slice(&char_mesh.uvs);
    let first = all.indices.len() as u32;
    all.indices.extend(char_mesh.indices.iter().map(|i| i + base as u32));
    all.shapes.extend(char_mesh.shapes.iter().map(|s| {
        let mut s = s.clone();
        s.first_index += first;
        s
    }));
    textures.extend(char_tex);
    poe2_bundles::room::recompute_bbox(&mut all);
    let center = mesh_center(&all);

    renderer.set_model(&all, &textures);
    if let Some(sc) = renderer.scene.as_mut() {
        sc.radius = 100.0;
    }
    let clips: Vec<Clip> =
        ["idle_unarmed_01", "run_unarmed_01", "attack_hollowpalm_01a"].iter().filter_map(|n| load_clip(idx, dir, &format!("{set}:{n}"))).collect();
    renderer.set_animation(&char_mesh, clips, base, center);
    if let Some(w) = window {
        w.set_title(&format!("openPOE2 - {character} in {world} (WASD move, Space attack, drag orbit, wheel zoom)"));
    }
    Some(Play { keys: [false; 4], pos: start, heading: 0.0, center, running: false, attack: None, clip_time: 0.0, clip: 0 })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let window = Arc::new(
            el.create_window(Window::default_attributes().with_title("openPOE2").with_inner_size(winit::dpi::LogicalSize::new(1100, 800)))
                .expect("create window"),
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(el.owned_display_handle())));
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no graphics adapter");
        println!("adapter: {}", adapter.get_info().name);
        let (device, queue) = pollster::block_on(request_device(&adapter)).expect("device");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported by adapter");
        if let Some(f) = surface.get_capabilities(&adapter).formats.into_iter().find(|f| f.is_srgb()) {
            config.format = f;
        }
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(device, queue, config.format);
        if let Some(world) = self.world.clone() {
            self.play = show_play(&self.idx, &self.dir, &world, &self.list[self.pos], "baseunarmed", &mut renderer, Some(&window));
            self.cam.pitch = 0.6;
            self.auto_rotate = false;
        } else {
            show(&self.idx, &self.dir, &self.list, self.pos, self.anim_spec.as_deref(), &mut renderer, &window);
        }
        let depth = renderer.depth_view((config.width, config.height));
        self.state = Some(Window3d { window, surface, config, depth, renderer });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(st) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if self.play.is_some()
                    && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyW | KeyCode::KeyA | KeyCode::KeyS | KeyCode::KeyD)) =>
            {
                let slot = match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyW) => 0,
                    PhysicalKey::Code(KeyCode::KeyA) => 1,
                    PhysicalKey::Code(KeyCode::KeyS) => 2,
                    _ => 3,
                };
                if let Some(p) = self.play.as_mut() {
                    p.keys[slot] = event.state == ElementState::Pressed;
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if self.play.is_some() && event.state == ElementState::Pressed && event.physical_key == PhysicalKey::Code(KeyCode::Space) =>
            {
                if let Some(p) = self.play.as_mut() {
                    if p.attack.is_none() {
                        p.attack = Some(0.0);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key {
                Key::Named(NamedKey::Escape) => el.exit(),
                Key::Named(NamedKey::Space) => self.auto_rotate = !self.auto_rotate,
                Key::Named(k @ (NamedKey::ArrowRight | NamedKey::ArrowLeft | NamedKey::PageDown | NamedKey::PageUp | NamedKey::Home | NamedKey::End)) => {
                    let n = self.list.len();
                    self.pos = match k {
                        NamedKey::ArrowRight => (self.pos + 1) % n,
                        NamedKey::ArrowLeft => (self.pos + n - 1) % n,
                        NamedKey::PageDown => (self.pos + 10) % n,
                        NamedKey::PageUp => (self.pos + n - 10 % n) % n,
                        NamedKey::Home => 0,
                        _ => n - 1,
                    };
                    self.cam.zoom = 1.0;
                    show(&self.idx, &self.dir, &self.list, self.pos, self.anim_spec.as_deref(), &mut st.renderer, &st.window);
                }
                _ => {}
            },
            WindowEvent::MouseInput { button: MouseButton::Left, state, .. } => {
                self.dragging = state == ElementState::Pressed;
                self.last_cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging {
                    if let Some((lx, ly)) = self.last_cursor {
                        self.cam.yaw -= (position.x - lx) as f32 * 0.008;
                        self.cam.pitch = (self.cam.pitch + (position.y - ly) as f32 * 0.008).clamp(-1.5, 1.5);
                    }
                }
                self.last_cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.cam.zoom = (self.cam.zoom * (1.0 - d * 0.1)).clamp(0.2, 4.0);
            }
            WindowEvent::Resized(s) if s.width > 0 && s.height > 0 => {
                st.config.width = s.width;
                st.config.height = s.height;
                st.surface.configure(&st.renderer.device, &st.config);
                st.depth = st.renderer.depth_view((s.width, s.height));
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;
                self.clock += dt;
                if let Some(play) = self.play.as_mut() {
                    play.step(dt, &mut self.cam, self.clock, &mut st.renderer);
                } else {
                    st.renderer.animate(self.clock, Mat4::IDENTITY);
                }
                if self.auto_rotate && !self.dragging {
                    self.cam.yaw += dt * 0.5;
                }
                match st.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                        let view = frame.texture.create_view(&Default::default());
                        st.renderer.render(&view, &st.depth, (st.config.width, st.config.height), &self.cam);
                        st.window.pre_present_notify();
                        st.renderer.queue.present(frame);
                    }
                    _ => st.surface.configure(&st.renderer.device, &st.config),
                }
                st.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        a.len() >= 3,
        "usage: viewer <Bundles2> <model | @name-filter> [--start N] [--screenshot out.png [yaw]] [--size WxH]
                      [--anim set[:name]] [--time seconds] [--world room.arm|tile.tgt]
         with a filter, Left/Right step through matching models, PageUp/PageDown jump by 10, Home/End"
    );
    let dir = PathBuf::from(&a[1]);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;

    let mut list: Vec<String> = match a[2].strip_prefix('@') {
        Some(filter) => {
            let f = filter.to_ascii_lowercase();
            idx.paths.keys().filter(|p| (p.ends_with(".fmt") || p.ends_with(".smd") || p.ends_with(".tgt") || p.ends_with(".arm") || p.ends_with(".ao")) && p.contains(&f)).cloned().collect()
        }
        None => vec![a[2].clone()],
    };
    list.sort();
    anyhow::ensure!(!list.is_empty(), "no models match {}", a[2]);
    let start = a
        .iter()
        .position(|s| s == "--start")
        .and_then(|i| a.get(i + 1)?.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, list.len())
        - 1;
    println!("{} model(s)", list.len());

    let size = a
        .iter()
        .position(|s| s == "--size")
        .and_then(|i| a.get(i + 1))
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((1280, 800));

    if let Some(i) = a.iter().position(|s| s == "--screenshot") {
        let out = a.get(i + 1).context("--screenshot needs an output path")?;
        let yaw: f32 = a.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(30.0f32);
        let path = &list[start];
        if let Some(world) = a.iter().position(|s| s == "--world").and_then(|i| a.get(i + 1)) {
            let secs: f32 = a.iter().position(|s| s == "--run").and_then(|i| a.get(i + 1)?.parse().ok()).unwrap_or(0.0);
            return screenshot_world(&idx, &dir, world, path, secs, secs > 0.0, size, yaw.to_radians(), out);
        }
        let mesh = model::load_mesh(&idx, &dir, path).with_context(|| format!("loading {path}"))?;
        let textures = model::load_shape_textures(&idx, &dir, path, &mesh);
        let clip = a.iter().position(|s| s == "--anim").and_then(|i| a.get(i + 1)).and_then(|spec| load_clip(&idx, &dir, spec));
        let time: f32 = a.iter().position(|s| s == "--time").and_then(|i| a.get(i + 1)?.parse().ok()).unwrap_or(0.0);
        println!(
            "{path}: {} triangles, {} shapes, {} textured",
            mesh.indices.len() / 3,
            mesh.shapes.len(),
            textures.iter().flatten().count()
        );
        return screenshot(&mesh, &textures, clip, time, size, yaw.to_radians(), out);
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        idx,
        dir,
        list,
        pos: start,
        state: None,
        cam: Camera { yaw: 0.5, pitch: 0.25, zoom: 1.0, target: Vec3::ZERO },
        auto_rotate: true,
        dragging: false,
        last_cursor: None,
        last_frame: Instant::now(),
        anim_spec: a.iter().position(|s| s == "--anim").and_then(|i| a.get(i + 1)).cloned(),
        clock: 0.0,
        world: a.iter().position(|s| s == "--world").and_then(|i| a.get(i + 1)).cloned(),
        play: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
