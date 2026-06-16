use bark::ecs::{Commands, IntoSystem, Query, ResMut};
use bark::phase;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
        )
        .init();

    let mut app = bark::App::new();

    app.world.insert_system(phase::Startup, startup);
    app.world.insert_system(phase::Update, increment);
    app.world.insert_system(phase::Update, physics);
    app.world.insert_system(phase::Update, test.after(physics));

    app.world.run_schedule(phase::Startup);
    for _ in 0..10 {
        app.world.run_schedule(phase::Update);
    }
}

struct Frame(u64);
struct Position(u32, u32, u32);
struct Velocity(u32, u32, u32);
struct Marker;

fn startup(mut commands: Commands) {
    commands.insert_resource(Frame(0));
    for i in 0..1000 {
        let mut test = commands.spawn();
        test.insert(Position(i, i, i));
        test.insert(Velocity(i, i, i));
        if i % 500 == 0 {
            test.insert(Marker);
        }
    }
}

fn increment(mut frame: ResMut<Frame>, mut commands: Commands) {
    frame.0 += 1;
    for i in 0..100 {
        let mut test = commands.spawn();
        test.insert(Position(i, i, i));
        test.insert(Velocity(i, i, i));
    }
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
        velocity.0 /= 2;
        velocity.1 /= 2;
        velocity.2 /= 2;
    }
}
