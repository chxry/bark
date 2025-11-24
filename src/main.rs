use bark::assets::Assets;
use bark::bark3d::mesh::MeshSource;
use bark::bark3d::texture::TextureSource;
use bark::bark3d::{self, Camera, PbrMode, RenderObject, Transform};
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
    world.insert_system_with(bark3d::init, scene);
    world.insert_system_with(app::update, update);
    world.queue_and_run(bark3d::bark3d);
}

struct Spin;

fn scene(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();

    let plant_pot_mesh = assets.load("assets/potted_plant_02_pot.obj");
    let plant_pot_diffuse = assets.load("assets/potted_plant_02_pot_diff_4k.png");
    let plant_pot_normal = assets.load("assets/potted_plant_02_pot_nor_gl_4k.png");
    let plant_pot_pbr = assets.load("assets/potted_plant_02_pot_arm_4k.png");

    let plant_leaves_mesh = assets.load("assets/potted_plant_02_leaves.obj");
    let plant_leaves_diffuse = assets.load("assets/potted_plant_02_leaves_diff_4k.png");
    let plant_leaves_normal = assets.load("assets/potted_plant_02_leaves_nor_gl_4k.png");
    let plant_leaves_pbr = assets.load("assets/potted_plant_02_leaves_arm_4k.png");

    let garfield_mesh = assets.load("assets/garfield.obj");
    let garfield_diffuse = assets.load("assets/garfield.png");

    let plane_mesh = assets.load("assets/plane.obj");

    world
        .spawn()
        .insert(Transform {
            position: Vec3::new(0.0, 5.0, 10.0),
            rotation: Quat::from_rotation_x(-0.1),
            scale: Vec3::ONE,
        })
        .insert(Camera { fov: 1.1 });
    world
        .spawn()
        .insert(Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.5),
        })
        .insert(RenderObject {
            mesh: MeshSource::new(plant_pot_mesh),
            diffuse_colour: Vec3::ONE,
            diffuse: Some(TextureSource::new(plant_pot_diffuse, true)),
            normal: Some(TextureSource::new(plant_pot_normal, false)),
            pbr: PbrMode::Sampled(TextureSource::new(plant_pot_pbr, false)),
        })
        .insert(Spin);
    world
        .spawn()
        .insert(Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.5),
        })
        .insert(RenderObject {
            mesh: MeshSource::new(plant_leaves_mesh),
            diffuse_colour: Vec3::ONE,
            diffuse: Some(TextureSource::new(plant_leaves_diffuse, true)),
            normal: Some(TextureSource::new(plant_leaves_normal, false)),
            pbr: PbrMode::Sampled(TextureSource::new(plant_leaves_pbr, false)),
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
            diffuse_colour: Vec3::ONE,
            diffuse: Some(TextureSource::new(garfield_diffuse.clone(), true)),
            normal: None,
            pbr: PbrMode::Values {
                roughness: 0.5,
                metallic: 0.0,
            },
        });
    world
        .spawn()
        .insert(Transform {
            position: Vec3::new(0.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(10.0),
        })
        .insert(RenderObject {
            mesh: MeshSource::new(plane_mesh),
            diffuse_colour: Vec3::splat(0.95),
            diffuse: None,
            normal: None,
            pbr: PbrMode::Values {
                roughness: 0.0,
                metallic: 0.0,
            },
        });
}

fn scene2(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();

    let sphere_mesh = assets.load("assets/sphere.obj");

    let rows = 10;
    let cols = 10;
    let spacing = 1.25;
    let scale = 0.5;

    for y in 0..rows {
        for x in 0..cols {
            let roughness = x as f32 / rows as f32;
            let metallic = y as f32 / cols as f32;

            world
                .spawn()
                .insert(Transform {
                    position: Vec3::new(x as f32 * spacing, y as f32 * spacing, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::splat(scale),
                })
                .insert(RenderObject {
                    mesh: MeshSource::new(sphere_mesh.clone()),
                    diffuse_colour: Vec3::new(1.0, 0.0, 0.0),
                    diffuse: None,
                    normal: None,
                    pbr: PbrMode::Values {
                        roughness,
                        metallic,
                    },
                });
        }
    }

    world
        .spawn()
        .insert(Transform {
            position: Vec3::new(
                (rows as f32 / 2.0) * spacing - scale,
                (cols as f32 / 2.0) * spacing - scale,
                10.0,
            ),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })
        .insert(Camera { fov: 1.2 });
}

fn update(world: &mut World) {
    for (_, (t, _)) in intersect(world.get_mut::<Transform>(), world.get::<Spin>()) {
        t.rotation *= Quat::from_rotation_y(0.01);
    }
}
