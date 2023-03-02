use lazy_static::lazy_static;

use slabmalloc::GLOBAL_PAGE_POOL;

lazy_static! {
    pub(crate) static ref PAGE_RECLAIMER_CTX: PageReclaimerContext =
        PageReclaimerContext::initialize();
}

pub(crate) struct PageReclaimerContext;

impl PageReclaimerContext {
    const SMALL_PAGE_RELEASE_THRESHOLD: usize = 4096;
    const SMALL_PAGE_RELEASE_RESERVE: usize = 2048;
    const LARGE_PAGE_RELEASE_THRESHOLD: usize = 256;
    const LARGE_PAGE_RELEASE_RESERVE: usize = 128;
    const HUGE_PAGE_RELEASE_THRESHOLD: usize = 0;
    const HUGE_PAGE_RELEASE_RESERVE: usize = 0;
    const RELEASE_INTERVAL_MS: u64 = 5000;

    async fn reclaim_task() {
        loop {
            GLOBAL_PAGE_POOL.release_small_pages(
                Self::SMALL_PAGE_RELEASE_THRESHOLD,
                Self::SMALL_PAGE_RELEASE_RESERVE,
            );
            GLOBAL_PAGE_POOL.release_large_pages(
                Self::LARGE_PAGE_RELEASE_THRESHOLD,
                Self::LARGE_PAGE_RELEASE_RESERVE,
            );
            GLOBAL_PAGE_POOL.release_huge_pages(
                Self::HUGE_PAGE_RELEASE_THRESHOLD,
                Self::HUGE_PAGE_RELEASE_RESERVE,
            );
            smol::Timer::after(std::time::Duration::from_millis(Self::RELEASE_INTERVAL_MS)).await;
        }
    }

    fn initialize() -> PageReclaimerContext {
        lazy_static::initialize(&GLOBAL_PAGE_POOL);
        let task = Self::reclaim_task();
        std::thread::spawn(move || smol::future::block_on(task));
        PageReclaimerContext
    }
}
