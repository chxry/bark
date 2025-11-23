use bark::assets::Assets;
use bark::bark3d::mesh::MeshSource;
use bark::bark3d::texture::TextureSource;
use bark::bark3d::{self, RenderObject, Transform};
use bark::ecs::World;
use bark::{app, intersect};
use glam::{Quat, Vec3};
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
        )
        .init();

    let mut world = World::default();
    world.insert_system_with(bark3d::init, setup);
    world.insert_system_with(app::update, update);
    world.queue_and_run(bark3d::bark3d);
}

struct Spin;

fn setup(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();

    let plant_pot_mesh = assets.load("assets/potted_plant_02_pot.obj");
    let plant_pot_diffuse = assets.load("assets/potted_plant_02_pot_diff_4k.png");
    let plant_pot_normal = assets.load("assets/potted_plant_02_pot_nor_gl_4k.png");

    let plant_leaves_mesh = assets.load("assets/potted_plant_02_leaves.obj");
    let plant_leaves_diffuse = assets.load("assets/potted_plant_02_leaves_diff_4k.png");
    let plant_leaves_normal = assets.load("assets/potted_plant_02_leaves_nor_gl_4k.png");

    let garfield_mesh = assets.load("assets/garfield.obj");
    let garfield_diffuse = assets.load("assets/garfield.png");

    world
        .spawn()
        .insert(Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(5.0),
        })
        .insert(RenderObject {
            mesh: MeshSource::new(plant_pot_mesh),
            diffuse: TextureSource::new(plant_pot_diffuse, true),
            normal: Some(TextureSource::new(plant_pot_normal, false)),
        })
        .insert(Spin);
    world
        .spawn()
        .insert(Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(5.0),
        })
        .insert(RenderObject {
            mesh: MeshSource::new(plant_leaves_mesh),
            diffuse: TextureSource::new(plant_leaves_diffuse, true),

            normal: Some(TextureSource::new(plant_leaves_normal, false)),
        })
        .insert(Spin);
    world
        .spawn()
        .insert(Transform {
            position: Vec3::new(-4.0, 0.0, -4.0),
            rotation: Quat::from_rotation_y(-0.5),
            scale: Vec3::ONE,
        })
        .insert(RenderObject {
            mesh: MeshSource::new(garfield_mesh),
            diffuse: TextureSource::new(garfield_diffuse, true),
            normal: None,
        });
}

fn update(world: &mut World) {
    for (_, (t, _)) in intersect(world.get_mut::<Transform>(), world.get::<Spin>()) {
        t.rotation *= Quat::from_rotation_y(0.01);
    }
}
