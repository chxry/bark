use crate::app::DeltaTime;
use crate::assets::Assets;
use crate::ecs::{Commands, IntoSystem, Res, ResMut, System};
use crate::gfx::{self, DEFAULT_BUFFER_SIZE, RenderContext, SURFACE_FORMAT, load_shader};
use crate::{App, app, bark3d};
use std::mem;
use std::ptr::NonNull;

pub use dear_imgui_rs as imgui;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(init_ui_pipeline.after(gfx::init_renderer));
    app.world
        .insert_system::<app::Render>(begin_ui.with(gfx::during_frame));
    app.world
        .insert_system::<app::Render>(draw_ui.after(begin_ui).after(bark3d::render::main_pass));
}

pub struct UiContext {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    render_pipeline: wgpu::RenderPipeline,
    imgui_ctx: imgui::Context,
    current_frame: Option<NonNull<imgui::Ui>>,
}

// safety: all methods must only access `imgui_ctx`/`current_frame` with `&mut Self`
unsafe impl Send for UiContext {}
unsafe impl Sync for UiContext {}

impl UiContext {
    pub fn frame(&mut self) -> &mut imgui::Ui {
        unsafe { self.current_frame.unwrap().as_mut() }
    }
}

fn init_ui_pipeline(
    render_ctx: Res<RenderContext>,
    mut assets: ResMut<Assets>,
    mut commands: Commands,
    _: crate::ecs::MainThread,
) {
    let vertex_buf = render_ctx.device.create_buffer(&wgpu::BufferDescriptor {
        size: DEFAULT_BUFFER_SIZE,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        label: None,
    });
    let index_buf = render_ctx.device.create_buffer(&wgpu::BufferDescriptor {
        size: DEFAULT_BUFFER_SIZE,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        label: None,
    });

    let pipeline_layout =
        render_ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[],
                immediate_size: 0,
            });
    let shader = load_shader(&render_ctx.device, &mut assets, "shaders/ui.wesl");
    let render_pipeline =
        render_ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_ui"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<imgui::DrawVert>() as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes:  &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_ui"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SURFACE_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

    let mut imgui_ctx = imgui::Context::create();
    let io = imgui_ctx.io_mut();
    io.backend_flags().insert(
        imgui::BackendFlags::HAS_MOUSE_CURSORS
            | imgui::BackendFlags::HAS_SET_MOUSE_POS
            | imgui::BackendFlags::RENDERER_HAS_VTX_OFFSET
            | imgui::BackendFlags::RENDERER_HAS_TEXTURES,
    );

    let surface_config = render_ctx.surface.get_configuration().unwrap();
    io.set_display_size([surface_config.width as _, surface_config.height as _]);

    commands.insert_resource(UiContext {
        vertex_buf,
        index_buf,
        render_pipeline,
        imgui_ctx,
        current_frame: None,
    });
}

fn begin_ui(mut ui_ctx: ResMut<UiContext>, dt: Res<DeltaTime>, _: crate::ecs::MainThread) {
    ui_ctx.imgui_ctx.io_mut().set_delta_time(dt.0.as_secs_f32());
    ui_ctx.current_frame = Some(NonNull::from_mut(ui_ctx.imgui_ctx.frame()));
}

fn draw_ui(mut ui_ctx: ResMut<UiContext>, _: crate::ecs::MainThread) {
    ui_ctx.current_frame = None;
    let draw_data = ui_ctx.imgui_ctx.render();
}

pub fn ui_frame(sys: Box<dyn System>) -> Box<dyn System> {
    sys.after(begin_ui).before(draw_ui)
}
