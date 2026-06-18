use bark::app::{self, ResizeEvent};
use bark::ecs::{Commands, Events, Query};
use bark::gfx;
use tracing::{info, trace};
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
        )
        .init();

    let mut app = bark::App::new();
    gfx::init(&mut app);
    app.world.insert_system(app::Startup, spawn);
    // app.world.insert_system(app::Update, physics);
    // app.world.insert_system(app::Update, cull.after(physics));
    app.world.insert_system(app::Update, read_events);

    app.run();
}

struct Position(u32, u32, u32);
struct Velocity(u32, u32, u32);

fn spawn(mut commands: Commands) {
    for i in 0..10000 {
        let mut test = commands.spawn();
        test.insert(Position(i, i, i));
        test.insert(Velocity(i, i, i));
    }
}

fn physics(mut query: Query<(&mut Position, &Velocity)>) {
    let mut n = 0;
    for (_, (position, velocity)) in query.iter() {
        position.0 += velocity.0;
        position.1 += velocity.1;
        position.2 += velocity.2;
        n += 1;
    }
    trace!("physics n={}", n);
}

fn cull(mut query: Query<(&Position,)>, mut commands: Commands) {
    let mut n = 0;
    for (e, (position,)) in query.iter() {
        if position.0 > 95000 {
            commands.entity(e).despawn();
            n += 1;
        }
    }
    info!("cull n={}", n);
}

fn read_events(events: Events<ResizeEvent>) {
    if let Some(event) = events.iter().last() {
        info!("resized! width={} height={}", event.width, event.height);
    }
}
