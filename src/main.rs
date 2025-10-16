use bark::ecs::World;
use bark::{app, gfx};
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
        )
        .init();

    let mut world = World::default();
    world.insert_resource("hi");
    world.insert_system_with(bark::init, app::init);
    world.insert_system_with(bark::init, gfx::init);
    world.insert_system_with(bark::init, test);
    world.insert_system_before(gfx::submit_frame, main_pass);
    world.run();
}

fn test(world: &mut World) {
    world.spawn().insert(3);
    world.spawn().insert(4).insert(5.0);

    info!("{:?}", world.get::<i32>().collect::<Vec<_>>());
    info!("{:?}", world.get::<f64>().collect::<Vec<_>>());
    info!("{:?}", world.get_resource::<&str>());
}

fn main_pass(world: &mut World) {
    let frame = world.get_resource_mut::<gfx::RenderFrame>().unwrap();

    frame
        .encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.surface_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.0,
                        b: 0.1,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            label: None,
        });
}
