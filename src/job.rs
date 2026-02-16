use crate::ecs::World;
use std::sync::{Arc, mpmc};
use std::thread;
use tracing::debug;

pub fn init(world: &mut World) {
    world.insert_resource(Arc::new(ThreadPool::new()));
}

pub trait Job = FnOnce() + Send + 'static;

pub struct ThreadPool {
    send: mpmc::Sender<Box<dyn Job>>,
}

impl ThreadPool {
    pub fn new() -> Self {
        let thread_count = thread::available_parallelism().unwrap();
        debug!("spawning {} worker threads", thread_count);
        let (send, recv) = mpmc::channel();
        for _ in 0..thread_count.into() {
            let recv2 = recv.clone();
            thread::spawn(move || {
                loop {
                    let job: Box<dyn Job> = recv2.recv().unwrap();
                    job();
                }
            });
        }

        Self { send }
    }

    pub fn execute<J: Job>(&self, f: J) {
        self.send.send(Box::new(f)).unwrap();
    }

    // todo have a way to gracefully shutdown the thread pool, and block app exit
}
