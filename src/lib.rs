use cgmath::Zero;
use wgpu::util::DeviceExt;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, Event, KeyEvent, MouseButton, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowBuilder},
};

mod screenshot;
mod texture;

use screenshot::screenshot;
use texture::Texture;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

    pub const BUFFER_LAYOUT: wgpu::VertexBufferLayout<'_> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as _,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &Vertex::ATTRIBS,
    };
}

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    projection_matrix: [[f32; 4]; 4],
    mouse_position: [f32; 2],
    flashlight: u32, // used as bool
    flashlight_radius: f32,
    _padding: f32,
}

#[allow(unused)]
struct State<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,

    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,

    uniform: Uniform,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,

    texture_bind_group: wgpu::BindGroup,
    texture: Texture,

    ctrl_key_held: bool,

    // camera stuff
    camera_velocity: f32,
    camera_target: cgmath::Vector2<f32>, // origin
    camera_zoom: f32,
    click_start_position: Option<cgmath::Vector2<f32>>,
    last_mouse_position: cgmath::Vector2<f32>,
    flashlight_radius_velocity: f32,
    //

    window: &'a Window,
}

impl<'a> State<'a> {
    // Creating some of the wgpu types requires async code
    async fn new(window: &'a Window, screenshot: ((u32, u32), Vec<u8>)) -> State<'a> {
        let ((width, height), _) = screenshot;

        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL, // low FPS in vulkan
            ..Default::default()
        });

        let surface = instance.create_surface(window).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptionsBase {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: None,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let (dimensions, data) = screenshot;
        let texture =
            Texture::from_bytes(&device, &queue, &data, dimensions, "screenshot texture").unwrap();

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
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

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&texture.sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let uniform = Uniform {
            projection_matrix: cgmath::ortho(0.0, width as _, 0.0, height as _, -1.0, 1.0).into(),
            mouse_position: [0.0; 2],
            flashlight: 0,
            flashlight_radius: 130.0,
            _padding: 0.0,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform buffer"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&uniform_bind_group_layout, &texture_bind_group_layout],
                ..Default::default()
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::BUFFER_LAYOUT],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let sw = width as f32;
        let sh = height as f32;

        #[rustfmt::skip]
        let vertices: &[Vertex] = &[
            Vertex { position: [ 0.0,  0.0, 0.0], tex_coords: [0.0, 0.0] }, // bottom left
            Vertex { position: [  sw,  0.0, 0.0], tex_coords: [1.0, 0.0] }, // bottom right
            Vertex { position: [  sw,   sh, 0.0], tex_coords: [1.0, 1.0] }, // top right
            Vertex { position: [ 0.0,   sh, 0.0], tex_coords: [0.0, 1.0] }, // top left
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            render_pipeline,
            vertex_buffer,
            index_buffer,

            uniform,
            uniform_bind_group,
            uniform_buffer,

            camera_velocity: 0.0,
            camera_target: cgmath::Vector2 { x: 0.0, y: 0.0 },
            camera_zoom: 1.0,
            click_start_position: None,
            last_mouse_position: cgmath::Vector2::zero(),
            flashlight_radius_velocity: 0.0,

            texture_bind_group,
            texture,

            ctrl_key_held: false,

            surface,
            device,
            queue,
            config,
            size,

            window,
        }
    }

    pub fn window(&self) -> &Window {
        self.window
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn input(&mut self, event: &WindowEvent) -> bool {
        const CAMERA_ACCELERATION: f32 = 0.01;

        match event {
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                self.last_mouse_position = cgmath::Vector2::new(*x as _, *y as _);
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.click_start_position = Some(self.last_mouse_position);
                self.window
                    .set_cursor_icon(winit::window::CursorIcon::Grabbing);
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.click_start_position = None;
                self.window
                    .set_cursor_icon(winit::window::CursorIcon::Default);
            }

            WindowEvent::MouseWheel {
                delta: winit::event::MouseScrollDelta::LineDelta(_, y),
                ..
            } => {
                if self.ctrl_key_held {
                    self.flashlight_radius_velocity += CAMERA_ACCELERATION * y * 200.0;
                } else {
                    self.camera_velocity += CAMERA_ACCELERATION * y
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key:
                            PhysicalKey::Code(KeyCode::ControlLeft | KeyCode::ControlRight),
                        ..
                    },
                ..
            } => self.ctrl_key_held = !self.ctrl_key_held,

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::KeyF),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.uniform.flashlight = (self.uniform.flashlight == 0) as _,

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::KeyR),
                        ..
                    },
                ..
            } => {
                self.camera_velocity = 0.0;
                self.camera_target = cgmath::Vector2::zero();
                self.camera_zoom = 1.0;
            }

            _ => return false,
        }

        true
    }

    fn update(&mut self) {
        if let Some(start_pos) = self.click_start_position {
            let displacement = self.last_mouse_position - start_pos;

            self.camera_target += displacement;

            let PhysicalSize { width, height } = self.window.inner_size();
            let sw = width as f32;
            let sh = height as f32;

            self.last_mouse_position = match self.last_mouse_position {
                cgmath::Vector2 { x: 0.0, y } => cgmath::Vector2::new(sw as _, y),
                cgmath::Vector2 { x, y: 0.0 } => cgmath::Vector2::new(x, sh as _),
                cgmath::Vector2 { x, y } if x + 1.0 >= sw => cgmath::Vector2::new(1.0, y),
                cgmath::Vector2 { x, y } if y + 1.0 >= sh => cgmath::Vector2::new(x, 0.0),

                x => x,
            };

            if self.last_mouse_position.x == 0.0 {
                self.last_mouse_position = cgmath::Vector2::new(sw, self.last_mouse_position.y);
            }

            self.click_start_position = Some(self.last_mouse_position);
            self.window
                .set_cursor_position(PhysicalPosition::new(
                    self.last_mouse_position.x,
                    self.last_mouse_position.y,
                ))
                .unwrap();
        }

        self.flashlight_radius_velocity *= 0.9;
        self.camera_velocity *= 0.95;

        self.uniform.flashlight_radius += self.flashlight_radius_velocity;
        self.uniform.flashlight_radius = self.uniform.flashlight_radius.clamp(30.0, 1000.0);
        self.camera_zoom += self.camera_velocity;
        self.camera_zoom = self.camera_zoom.clamp(0.01, 100.0);

        let s = self.window.inner_size();
        let sw = s.width as f32;
        let sh = s.height as f32;

        let center_x = sw / 2.0;
        let center_y = sh / 2.0;

        let z = self.camera_zoom;
        let o = self.camera_target;

        let left = center_x - (center_x / z) - o.x / z;
        let right = center_x + (center_x / z) - o.x / z;
        let bottom = center_y - (center_y / z) + o.y / z;
        let top = center_y + (center_y / z) + o.y / z;

        self.uniform.projection_matrix = cgmath::ortho(left, right, bottom, top, -1.0, 1.0).into();
        self.uniform.mouse_position = self.last_mouse_position.into();

        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniform]),
        );
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.009,
                        g: 0.009,
                        b: 0.009,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_bind_group(1, &self.texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..INDICES.len() as _, 0, 0..1);

        drop(render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

pub async fn run() {
    let screenshot = screenshot();

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(
            event_loop.primary_monitor(),
        )))
        .build(&event_loop)
        .unwrap();

    let mut state = State::new(&window, screenshot).await;

    event_loop
        .run(move |event, control_flow| {
            #[allow(clippy::single_match)]
            match event {
                Event::WindowEvent {
                    window_id,
                    ref event,
                } if window_id == state.window.id() => {
                    if !state.input(event) {
                        match event {
                            WindowEvent::Resized(physical_size) => state.resize(*physical_size),
                            WindowEvent::CloseRequested
                            | WindowEvent::KeyboardInput {
                                event:
                                    KeyEvent {
                                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                                        ..
                                    },
                                ..
                            } => control_flow.exit(),

                            WindowEvent::RedrawRequested => {
                                state.window().request_redraw();

                                state.update();
                                match state.render() {
                                    Ok(_) => {}

                                    Err(
                                        wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated,
                                    ) => state.resize(state.size),

                                    Err(wgpu::SurfaceError::OutOfMemory) => {
                                        eprintln!("[Error] OutOfMemory");
                                        control_flow.exit();
                                    }

                                    Err(wgpu::SurfaceError::Timeout) => {
                                        eprintln!("[Error] Surface timeout")
                                    }

                                    _ => panic!("Unknown error"),
                                }
                            }

                            _ => {}
                        }
                    }
                }

                _ => {}
            }
        })
        .unwrap();
}
