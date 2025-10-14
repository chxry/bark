use bark::ecs::World;
use tracing::info;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

fn main() {
    tracing_subscriber::registry()
        .with(LevelFilter::TRACE)
        .with(tracing_subscriber::fmt::layer())
        .init();

    bark::App::default()
        .insert_resource("hi")
        .insert_system_with(bark::init, test)
        .insert_system_before(test, pre)
        .run();
}

fn test(world: &mut World) {
    info!("{:?}", world.get::<i32>());
    info!("{:?}", world.get::<f64>());
    info!("{:?}", world.get_resource::<&str>());

    world.run_system(post);
}

fn post(world: &mut World) {
    info!("post");
}

fn pre(world: &mut World) {
    world.spawn().insert(3).insert(5.0);
    world.spawn().insert(4);
}
