use bark::app::winit::keyboard::KeyCode;
use bark::app::winit::window::CursorGrabMode;
use bark::app::{Input, WindowHandle};
use bark::bark3d::{self, Camera, DirectionalLight, Material, Parent, StaticMesh, Transform};
use bark::ecs::{Commands, IntoSystem, Query, Res, ResMut};
use bark::gfx::mesh::MeshManager;
use bark::gfx::texture::TextureManager;
use bark::math::{EulerRot, Quat, Vec3};
use bark::{app, assets, gfx};
use std::f32::consts::FRAC_PI_2;

fn main() {
    let assets_dir = env!("CARGO_MANIFEST_DIR").to_owned() + "/assets";
    let mut app = bark::App::new();
    gfx::init(&mut app);
    assets::init(&mut app, assets_dir);
    bark3d::init(&mut app);
    app.world.insert_system::<app::Startup>(scene.into_system());
    app.world.insert_system::<app::Update>(spinny.into_system());
    app.world
        .insert_system::<app::Update>(update_camera.into_system());
    app.run();
}

struct Spin;

fn scene(
    mut textures: ResMut<TextureManager>,
    mut meshes: ResMut<MeshManager>,
    mut commands: Commands,
) {
    let pot = commands
        .spawn()
        .insert(
            Transform::default()
                .position(Vec3::new(-2.0, 0.0, 0.0))
                .scale(Vec3::splat(2.0)),
        )
        .insert(Spin)
        .id();
    commands
        .spawn()
        .insert(Transform::default())
        .insert(Parent(pot))
        .insert(StaticMesh(meshes.add("potted_plant_02_pot.fbx", 0)))
        .insert(
            Material::default()
                .diffuse_texture(textures.add("potted_plant_02_pot_diff_4k.png"))
                .normal_texture(textures.add("potted_plant_02_pot_nor_gl_4k.png"))
                .pbr_texture(textures.add("potted_plant_02_pot_arm_4k.png")),
        );
    commands
        .spawn()
        .insert(Transform::default())
        .insert(Parent(pot))
        .insert(StaticMesh(meshes.add("potted_plant_02_leaves.fbx", 0)))
        .insert(
            Material::default()
                .diffuse_texture(textures.add("potted_plant_02_leaves_diff_4k.png"))
                .normal_texture(textures.add("potted_plant_02_leaves_nor_gl_4k.png"))
                .pbr_texture(textures.add("potted_plant_02_leaves_arm_4k.png")),
        );

    commands
        .spawn()
        .insert(
            Transform::default()
                .position(Vec3::new(2.0, 0.0, 0.0))
                .scale(Vec3::splat(0.2)),
        )
        .insert(StaticMesh(meshes.add("garfield.obj", 0)))
        .insert(Material::default().diffuse_texture(textures.add("garfield.png")));
    commands
        .spawn()
        .insert(Transform::default().scale(Vec3::splat(25.0)))
        .insert(StaticMesh(meshes.add("plane.obj", 0)))
        .insert(Material::default());
    commands
        .spawn()
        .insert(Transform::default().position(Vec3::new(0.0, 2.5, 5.0)))
        .insert(Camera::new(0.9))
        .insert(CameraController {
            yaw: 0.0,
            pitch: -0.2,
        });
}

fn scene2(
    mut meshes: ResMut<MeshManager>,
    mut textures: ResMut<TextureManager>,
    mut commands: Commands,
) {
    let mesh = meshes.add("sphere.obj", 0);
    // let mesh = meshes.add("garfield.obj", 0);
    let tex = textures.add("garfield.png");

    let rows = 10;
    let cols = 10;
    let spacing = 1.25;
    let scale = 0.5;
    // let scale = 0.25;

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
                .insert(StaticMesh(mesh))
                .insert(
                    Material::default()
                        .diffuse_color(Vec3::X)
                        // .diffuse_texture(tex)
                        .pbr_values(roughness, metallic),
                )
                .insert(Spin);
        }
    }

    commands
        .spawn()
        .insert(Transform::default().position(Vec3::new(
            (rows as f32 / 2.0) * spacing - scale,
            (cols as f32 / 2.0) * spacing - scale,
            10.0,
        )))
        .insert(Camera::new(1.2))
        .insert(CameraController {
            yaw: 0.0,
            pitch: 0.0,
        });

    commands
        .spawn()
        .insert(Transform::default().rotation_euler(1.5, -0.5, 0.0))
        .insert(DirectionalLight::default());
    commands
        .spawn()
        .insert(Transform::default())
        .insert(DirectionalLight::default().color(Vec3::splat(2.5)))
        .insert(Spin);
}

fn spinny(transforms: Query<(&mut Transform, &Spin)>) {
    for (_, (t, _)) in transforms.iter() {
        t.rotation = Quat::from_rotation_y(0.01) * t.rotation;
    }
}

struct CameraController {
    yaw: f32,
    pitch: f32,
}

fn update_camera(
    camera: Query<(&mut Transform, &mut CameraController)>,
    input: Res<Input>,
    window: Res<WindowHandle>,
) {
    let Some((_, (transform, controller))) = camera.iter().next() else {
        return;
    };

    let speed = if input.key_down(KeyCode::ShiftLeft) {
        0.01
    } else {
        0.1
    };
    let mouse_sens = 0.002;

    if input.key_down(KeyCode::KeyW) {
        transform.position += transform.rotation * bark3d::FORWARD * speed;
    }
    if input.key_down(KeyCode::KeyS) {
        transform.position -= transform.rotation * bark3d::FORWARD * speed;
    }
    if input.key_down(KeyCode::KeyA) {
        transform.position -= transform.rotation * bark3d::RIGHT * speed;
    }
    if input.key_down(KeyCode::KeyD) {
        transform.position += transform.rotation * bark3d::RIGHT * speed;
    }
    window
        .set_cursor_grab(if input.left_mouse_down() {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        })
        .unwrap();
    window.set_cursor_visible(!input.left_mouse_down());
    if input.left_mouse_down() {
        controller.yaw -= input.mouse_delta().0 * mouse_sens;
        controller.pitch -= input.mouse_delta().1 * mouse_sens;
        controller.pitch = controller.pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, controller.yaw, controller.pitch, 0.0);
}
