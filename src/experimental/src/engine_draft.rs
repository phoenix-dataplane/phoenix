pub type Version = u32;

pub trait Engine {
    fn version() -> Version;
    fn check_compatible(v1: Version, v2: Version) -> bool;
    fn init();
    /// `run()` mush be non-blocking and short.
    fn run();
    fn dump();
    fn restore();
    fn destroy();
    fn enqueue();
    fn check_queue_len();
}

pub struct MailBox;

pub struct KoalaTransport {
    input_queue: RingBuffer,
    mailbox: MailBox,
}

impl Engine for KoalaTransport {
    fn version() {
        0
    }

    fn check_compatible(v1: Version, v2: Version) {
        true
    }

    fn init() {}

    fn run() {
        let wr = input_queue.try_pop();
        dosomething(wr);
    }
}

use spin::{Mutex, MutexGuard};

struct EngineRuntime {
    id: usize,
    engines: Vec<Box<dyn Engine>>,
    /// work stealing
    pending_engines: Mutex<Vec<Box<dyn Engine>>>,
}

impl EngineRuntime {
    fn new(id: usize) -> Self {
        EngineRuntime {
            id,
            engines: Vec::new(),
            pending_engines: Vec::new(),
        }
    }

    fn add_engine(&mut self, engine: Box<dyn Engine>) {
        self.pending_engines.lock().unwrap().push(engine);
    }

    fn start(&mut self) {
        let handle = std::thread::spawn(|| {
            set_affinity_for_current(self.id);
            self.mainloop();
        });
        handle
    }

    fn mainloop(&mut self) {
        loop {
            for engine in &mut self.engines {
                engine.run();
            }
            // steal work from other runtimes
            self.engines
                .append(&mut self.pending_engines.lock().unwrap());
        }
    }
}

fn main() {
    let mut runtimes: Vec<EngineRuntime> = Vec::new();
    for i in 0..8 {
        let runtime = EngineRuntime::new(i);
        runtimes.push(runtime);
    }

    let mut handles = Vec::new();
    for i in 0..8 {
        handles.push(runtimes[i].start());
    }

    for i in 0..8 {
        handles[i].join().unwrap();
    }
}
