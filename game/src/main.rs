use bark::ecs::{IntoSystem, Query, Res, ResMut};
use bark::phase;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=trace")),
        )
        .init();

    let mut app = bark::App::new();
    app.world.insert_system(phase::Startup, test);
    app.world.insert_system(phase::Startup, test2.before(test));

    let a = app.world.spawn();
    let b = app.world.spawn();
    let u32_store = app.world.create_store::<u32>();
    u32_store.insert(a, 45);
    u32_store.insert(b, 55);
    let f32_store = app.world.create_store::<f32>();
    f32_store.insert(a, 1.23);
    app.world.insert_resource("resource!");

    app.run();
}

// fn test(query: Query<(&u32,)>) {
fn test(mut query1: Query<(&u32, &f32)>, mut query2: Query<(&u32,)>) {
    for (a, b) in query1.iter() {
        tracing::info!("query1: {:?} {:?}", a, b);
    }
    for (a,) in query2.iter() {
        tracing::info!("query2: {:?}", a);
    }
}

fn test2(resource: Res<&str>) {
    tracing::info!("{:?}", *resource);
}
