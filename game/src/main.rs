use bark::assets::{AssetProcessors, Assets};
use bark::bark3d::{self, RenderObject, Transform};
use bark::ecs::{Commands, ResMut};
use bark::{app, assets, gfx};

// move out of game crate
fn process_assets() {
    let mut assets = AssetProcessors::new(env!("CARGO_MANIFEST_DIR").to_owned() + "/assets");
    gfx::init_build(&mut assets);
    assets.run();
}

fn main() {
    process_assets();
    let mut app = bark::App::new();
    gfx::init(&mut app);
    assets::init(&mut app, env!("CARGO_MANIFEST_DIR").to_owned() + "/assets");
    bark3d::init(&mut app);
    app.world.insert_system(app::Startup, spawn);

    app.run();
}

fn spawn(mut assets: ResMut<Assets>, mut commands: Commands) {
    commands
        .spawn()
        .insert(Transform::default())
        .insert(RenderObject {
            mesh: assets.load("potted_plant_02_leaves.obj"),
            texture: assets.load("potted_plant_02_leaves_diff_4k.png"),
        });
    commands
        .spawn()
        .insert(Transform::default())
        .insert(RenderObject {
            mesh: assets.load("potted_plant_02_pot.obj"),
            texture: assets.load("potted_plant_02_pot_diff_4k.png"),
        });
}
