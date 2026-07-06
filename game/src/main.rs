use bark::assets::Assets;
use bark::bark3d::{self, Camera, DirectionalLight, RenderObject, Transform};
use bark::ecs::{Commands, IntoSystem, Query, ResMut};
use bark::gfx::mesh::MeshManager;
use bark::gfx::texture::TextureManager;
use bark::math::{Quat, Vec3};
use bark::{app, assets, gfx};
use std::process::Command;

fn main() {
    let assets_dir = env!("CARGO_MANIFEST_DIR").to_owned() + "/assets";
    Command::new("cargo")
        .args(["run", "--release", "--bin", "bark-build", &assets_dir])
        .status()
        .unwrap();

    let mut app = bark::App::new();
    gfx::init(&mut app);
    assets::init(&mut app, assets_dir);
    bark3d::init(&mut app);
    app.world.insert_system::<app::Startup>(scene.into_system());
    app.world.insert_system::<app::Update>(spinny.into_system());
    app.run();
}

struct Spin;

fn scene(
    mut assets: ResMut<Assets>,
    mut textures: ResMut<TextureManager>,
    mut meshes: ResMut<MeshManager>,
    mut commands: Commands,
) {
    commands.spawn().insert(Transform::default()).insert(
        RenderObject::new(meshes.add(assets.load("potted_plant_02_leaves.obj")))
            .diffuse_texture(textures.add(assets.load("potted_plant_02_leaves_diff_4k.png")))
            .normal_texture(textures.add(assets.load("potted_plant_02_leaves_nor_gl_4k.png")))
            .pbr_texture(textures.add(assets.load("potted_plant_02_leaves_arm_4k.png"))),
    );
    commands.spawn().insert(Transform::default()).insert(
        RenderObject::new(meshes.add(assets.load("potted_plant_02_pot.obj")))
            .diffuse_texture(textures.add(assets.load("potted_plant_02_pot_diff_4k.png")))
            .normal_texture(textures.add(assets.load("potted_plant_02_pot_nor_gl_4k.png")))
            .pbr_texture(textures.add(assets.load("potted_plant_02_pot_arm_4k.png"))),
    );
    commands
        .spawn()
        .insert(Transform::default().scale(Vec3::splat(10.0)))
        .insert(RenderObject::new(meshes.add(assets.load("plane.obj"))));
    commands
        .spawn()
        .insert(Transform::default().rotation_euler(0.0, -0.5, 0.0))
        .insert(DirectionalLight::default().shadows(true))
        .insert(Spin);
    commands
        .spawn()
        .insert(
            Transform::default()
                .position(Vec3::new(0.0, 2.5, 5.0))
                .rotation_euler(0.0, -0.2, 0.0),
        )
        .insert(Camera::new(0.9));
}

fn scene2(mut assets: ResMut<Assets>, mut meshes: ResMut<MeshManager>, mut commands: Commands) {
    let sphere_mesh = meshes.add(assets.load("sphere.obj"));

    let rows = 10;
    let cols = 10;
    let spacing = 1.25;
    let scale = 0.5;

    for y in 0..rows {
        for x in 0..cols {
            let roughness = x as f32 / (rows - 1) as f32;
            let metallic = 1.0 - y as f32 / (cols - 1) as f32;

            commands
                .spawn()
                .insert(
                    Transform::default()
                        .position(Vec3::new(x as f32 * spacing, y as f32 * spacing, 0.0))
                        .scale(Vec3::splat(scale)),
                )
                .insert(
                    RenderObject::new(sphere_mesh)
                        .diffuse_color(Vec3::X)
                        .pbr_values(roughness, metallic),
                );
        }
    }

    commands
        .spawn()
        .insert(Transform::default().position(Vec3::new(
            (rows as f32 / 2.0) * spacing - scale,
            (cols as f32 / 2.0) * spacing - scale,
            10.0,
        )))
        .insert(Camera::new(1.2));
    commands
        .spawn()
        .insert(Transform::default().rotation_euler(1.5, 0.0, 0.0))
        .insert(DirectionalLight::default());
    commands
        .spawn()
        .insert(Transform::default())
        .insert(DirectionalLight::default().shadows(true))
        .insert(Spin);
}

fn spinny(mut transforms: Query<(&mut Transform, &Spin)>) {
    for (_, (t, _)) in transforms.iter() {
        t.rotation = Quat::from_rotation_y(0.01) * t.rotation;
    }
}
