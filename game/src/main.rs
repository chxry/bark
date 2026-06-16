use bark::ecs::{IntoSystem, Query, ResMut};
use bark::phase;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,bark=debug")),
        )
        .init();

    let mut app = bark::App::new();

    app.world.insert_resource(Frame(0));
    app.world.insert_system(phase::Update, increment);
    app.world.insert_system(phase::Update, physics);
    app.world.insert_system(phase::Update, test.after(physics));

    for i in 0..1000000 {
        let e = app.world.spawn();

        app.world
            .create_store()
            .insert(e, Position(i as f32, i as f32, i as f32));
        app.world
            .create_store()
            .insert(e, Velocity(i as f32, i as f32, i as f32));
        if i % 50000 == 0 {
            app.world.create_store().insert(e, Marker);
        }
    }

    for _ in 0..100 {
        app.world.run_schedule(phase::Update);
    }
}

struct Frame(u64);
struct Position(f32, f32, f32);
struct Velocity(f32, f32, f32);
struct Marker;

fn increment(mut frame: ResMut<Frame>) {
    frame.0 += 1;
}

fn physics(mut query: Query<(&mut Position, &Velocity)>) {
    for (position, velocity) in query.iter() {
        position.0 += velocity.0;
        position.1 += velocity.1;
        position.2 += velocity.2;
    }
}

fn test(mut query: Query<(&Marker, &mut Velocity)>) {
    for (_, velocity) in query.iter() {
        velocity.0 /= 2.0;
        velocity.1 /= 2.0;
        velocity.2 /= 2.0;
    }
}
