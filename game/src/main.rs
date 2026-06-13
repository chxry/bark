use bark::ecs::{IntoSystem, Res, ResMut};
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
    let u32_store = app.world.get_store_mut::<u32>();
    u32_store.insert(a, 45);
    u32_store.insert(b, 55);
    let str_store = app.world.get_store_mut::<&str>();
    str_store.insert(a, "hmm");
    app.world.insert_resource("resource!");

    app.run();
}

fn test() {
    tracing::info!("!!");
}

fn test2(resource: Res<&str>) {
    tracing::info!("{:?}", *resource);
}
