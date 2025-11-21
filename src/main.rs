use bark::assets::Assets;
use bark::bark3d::{self, Mesh, MeshRenderer, Transform};
use bark::ecs::World;
use glam::{Mat4, Vec3};
use image::DynamicImage;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
        )
        .init();

    let mut world = World::default();
    world.insert_system_with(bark::init, bark3d::init);
    world.insert_system_with(bark3d::start, setup);
    world.run();
}

fn setup(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();
    let mesh = assets.load::<Mesh>("assets/garfield.obj");
    let texture = assets.load::<DynamicImage>("assets/garfield.png");

    world
        .spawn()
        .insert(Transform {
            mat: Mat4::IDENTITY,
        })
        .insert(MeshRenderer {
            mesh: mesh.clone(),
            texture: texture.clone(),
        });
    world
        .spawn()
        .insert(Transform {
            mat: Mat4::from_translation(Vec3::new(-3.0, 0.0, 0.0)),
        })
        .insert(MeshRenderer { mesh, texture });
}
