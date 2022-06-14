
pub struct GlobalShreadHeapPagePool {
    empty_small_pages: spin::Mutex<VecDeque<Unique<ObjectPage<'static>>>>,
    empty_large_pages: spin::Mutex<VecDeque<Unique<LargeObjectPage<'static>>>>,
    empty_huge_pages: spin::Mutex<VecDeque<Unique<HugeObjectPage<'static>>>>,

    used_small_pages: spin::Mutex<LinkedList<(Unique<ObjectPage<'static>>, usize)>>,
    used_large_pages: spin::Mutex<LinkedList<(Unique<LargeObjectPage<'static>>, usize)>>,
    used_huge_pages: spin::Mutex<LinkedList<(Unique<HugeObjectPage<'static>>, usize)>>,
}

impl GlobalShreadHeapPagePool {
    fn new() -> Self {
        GlobalShreadHeapPagePool {
            empty_small_pages: spin::Mutex::new(VecDeque::new()),
            empty_large_pages: spin::Mutex::new(VecDeque::new()),
            empty_huge_pages: spin::Mutex::new(VecDeque::new()),
            used_small_pages: spin::Mutex::new(LinkedList::new()),
            used_large_pages: spin::Mutex::new(LinkedList::new()),
            used_huge_pages: spin::Mutex::new(LinkedList::new()),
        }
    }

    fn check_small_page_assignments(&self) {
        let mut guard = self.used_small_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_small_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    fn check_large_page_assignments(&self) {
        let mut guard = self.used_large_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_large_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    fn check_huge_page_assignments(&self) {
        let mut guard = self.used_huge_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_huge_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    pub(crate) fn acquire_small_page(&self) -> Option<&'static mut ObjectPage<'static>> {
        // TODO(wyj): do we really need to check each time?
        self.check_small_page_assignments();
        let page = self
            .empty_small_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn acquire_large_page(&self) -> Option<&'static mut LargeObjectPage<'static>> {
        self.check_large_page_assignments();
        let page = self
            .empty_large_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn acquire_huge_page(&self) -> Option<&'static mut HugeObjectPage<'static>> {
        self.check_huge_page_assignments();
        let page = self
            .empty_huge_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn recycle_small_page(
        &self,
        page: &'static mut ObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_small_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_small_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }

    // Caller must ensure provided pages are empty
    // pub(crate) unsafe fn recycle_empty_small_pages<I: IntoIterator<Item = &'static mut ObjectPage<'static>>>(
    //     &self,
    //     pages: I,
    // ) {
    //     let pages = pages.into_iter().map(|page|
    //         unsafe { Unique::new_unchecked(page as *mut _) }
    //     );
    //     self.empty_small_pages.lock().extend(pages)
    // }

    pub(crate) fn recycle_large_page(
        &self,
        page: &'static mut LargeObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_large_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_large_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }

    pub(crate) fn recycle_huge_page(
        &self,
        page: &'static mut HugeObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_huge_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_huge_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }
}