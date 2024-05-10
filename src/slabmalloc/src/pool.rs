use alloc::collections::{LinkedList, VecDeque};
use core::ops::DerefMut;
use core::ptr::Unique;

use lazy_static::lazy_static;

use crate::{AllocablePage, HugeObjectPage, LargeObjectPage, ObjectPage};

lazy_static! {
    pub static ref GLOBAL_PAGE_POOL: GlobalPagePool<'static> = GlobalPagePool::new();
}

pub struct PageBuffer<P> {
    empty: VecDeque<Unique<P>>,
    used: LinkedList<(Unique<P>, usize)>,
}

impl<P> PageBuffer<P> {
    fn new() -> Self {
        PageBuffer {
            empty: VecDeque::new(),
            used: LinkedList::new(),
        }
    }
}

pub struct GlobalPagePool<'a> {
    small_pages: spin::Mutex<PageBuffer<ObjectPage<'a>>>,
    large_pages: spin::Mutex<PageBuffer<LargeObjectPage<'a>>>,
    huge_pages: spin::Mutex<PageBuffer<HugeObjectPage<'a>>>,
}

impl<'a> GlobalPagePool<'a> {
    pub(crate) fn new() -> Self {
        GlobalPagePool {
            small_pages: spin::Mutex::new(PageBuffer::new()),
            large_pages: spin::Mutex::new(PageBuffer::new()),
            huge_pages: spin::Mutex::new(PageBuffer::new()),
        }
    }

    pub fn acquire_small_page(&self) -> Option<&'a mut ObjectPage<'a>> {
        let mut guard = self.small_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }

        buf.empty
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() })
    }

    pub fn acquire_large_page(&self) -> Option<&'a mut LargeObjectPage<'a>> {
        let mut guard = self.large_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }

        buf.empty
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() })
    }

    pub fn acquire_huge_page(&self) -> Option<&'a mut HugeObjectPage<'a>> {
        let mut guard = self.huge_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }

        buf.empty
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() })
    }

    pub(crate) fn recycle_small_pages<I: IntoIterator<Item = &'a mut ObjectPage<'a>>>(
        &self,
        pages: I,
        obj_per_page: Option<usize>,
        empty: bool,
    ) {
        let mut guard = self.small_pages.lock();
        if empty {
            for page in pages.into_iter() {
                guard
                    .empty
                    .push_back(unsafe { Unique::new_unchecked(page as *mut _) });
            }
        } else {
            let obj_per_page = obj_per_page.unwrap();
            for page in pages.into_iter() {
                guard
                    .used
                    .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) });
            }
        }
    }

    pub(crate) fn recycle_large_pages<I: IntoIterator<Item = &'a mut LargeObjectPage<'a>>>(
        &self,
        pages: I,
        obj_per_page: Option<usize>,
        empty: bool,
    ) {
        let mut guard = self.large_pages.lock();
        if empty {
            for page in pages.into_iter() {
                guard
                    .empty
                    .push_back(unsafe { Unique::new_unchecked(page as *mut _) });
            }
        } else {
            let obj_per_page = obj_per_page.unwrap();
            for page in pages.into_iter() {
                guard
                    .used
                    .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) });
            }
        }
    }

    pub(crate) fn recycle_huge_pages<I: IntoIterator<Item = &'a mut HugeObjectPage<'a>>>(
        &self,
        pages: I,
        obj_per_page: Option<usize>,
        empty: bool,
    ) {
        let mut guard = self.huge_pages.lock();
        if empty {
            for page in pages.into_iter() {
                guard
                    .empty
                    .push_back(unsafe { Unique::new_unchecked(page as *mut _) });
            }
        } else {
            let obj_per_page = obj_per_page.unwrap();
            for page in pages.into_iter() {
                guard
                    .used
                    .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) });
            }
        }
    }

    pub fn release_small_pages(
        &self,
        threshold: usize,
        reserve: usize,
    ) -> Option<VecDeque<Unique<ObjectPage<'a>>>> {
        let mut guard = self.small_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }
        if buf.empty.len() > threshold {
            Some(buf.empty.split_off(reserve))
        } else {
            None
        }
    }

    pub fn release_large_pages(
        &self,
        threshold: usize,
        reserve: usize,
    ) -> Option<VecDeque<Unique<LargeObjectPage<'a>>>> {
        let mut guard = self.large_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }
        if buf.empty.len() > threshold {
            Some(buf.empty.split_off(reserve))
        } else {
            None
        }
    }

    pub fn release_huge_pages(
        &self,
        threshold: usize,
        reserve: usize,
    ) -> Option<VecDeque<Unique<HugeObjectPage<'a>>>> {
        let mut guard = self.huge_pages.lock();
        let buf = guard.deref_mut();
        let freed_pages = buf
            .used
            .extract_if(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });
        for (page, _) in freed_pages {
            buf.empty.push_back(page);
        }
        if buf.empty.len() > threshold {
            Some(buf.empty.split_off(reserve))
        } else {
            None
        }
    }
}
