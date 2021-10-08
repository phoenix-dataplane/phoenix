pub type Version = u32;

pub trait Engine {
    fn version(&self) -> Version;
    fn check_compatible(&self, other: Version) -> bool;
    fn init(&mut self);
    /// `run()` mush be non-blocking and short.
    fn run(&mut self);
    fn dump(&self);
    fn restore(&mut self);
    fn destroy(&self);
    fn enqueue(&self);
    fn check_queue_len(&self);
}