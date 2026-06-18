use bark::ecs::{Commands, Events, IntoSystem, Query, Res, ResMut};
use bark::phase;
use tracing::info;
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
    app.world.insert_system(phase::Update, cull.after(physics));
    app.world
        .insert_system(phase::Update, read_events.after(increment));

    app.world.run_schedule(phase::Startup);
    for _ in 0..100 {
        app.world.clear_events();
        app.world.run_schedule(phase::Update);
    }
}

struct Frame(u64);
struct Position(u32, u32, u32);
struct Velocity(u32, u32, u32);
struct Marker;
struct TestEvent(u64);

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
    commands.queue_event(TestEvent(frame.0));
    for i in 0..1000 {
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
    info!("physics n={}", n);
}

fn test(mut query: Query<(&Marker, &mut Velocity)>) {
    for (_, (_, velocity)) in query.iter() {
        velocity.0 /= 2;
        velocity.1 /= 2;
        velocity.2 /= 2;
    }
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

fn read_events(frame: Res<Frame>, events: Events<TestEvent>) {
    for e in events.iter() {
        info!("frame {}: receive event {}", frame.0, e.0);
    }
}
