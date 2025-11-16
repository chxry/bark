use bark::ecs::World;
use bark::{app, gfx, intersect};
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
    world.insert_system_before(gfx::submit_frame, main_pass);
    world.insert_system_with(bark::init, test);
    world.insert_system_with(app::update, update);
    world.run();
}

fn test(world: &mut World) {
    world
        .spawn()
        .insert(Position(0.0, 0.0))
        .insert(Velocity(1.0, 1.0));
    world
        .spawn()
        .insert(Position(5.0, 5.0))
        .insert(Velocity(-0.5, 0.0));
}

#[derive(Debug, Clone, Copy)]
struct Position(f32, f32);

#[derive(Debug, Clone, Copy)]
struct Velocity(f32, f32);

fn update(world: &mut World) {
    let positions = world.get_mut::<Position>();
    let velocities = world.get::<Velocity>();
    for (_, (p, v)) in intersect(positions, velocities) {
        p.0 += v.0;
        p.1 += v.1;
    }

    info!("{:?}", world.get::<Position>().collect::<Vec<_>>());
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
