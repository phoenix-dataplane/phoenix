use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::Deref;

use super::boxed::Box;

pub(crate) mod from_backend {
    pub trait CloneFromBackendOwned {
        type BackendOwned;

        fn clone_from_backend_owned(_: &Self::BackendOwned) -> Self;
    }
}

use from_backend::CloneFromBackendOwned;

#[derive(Clone, Copy)]
pub(crate) struct ShmRecvContext<'a>(PhantomData<&'a ()>);

impl<'a> ShmRecvContext<'a> {
    pub(crate) fn new<T>(_ctx: &'a T) -> Self {
        ShmRecvContext(PhantomData)
    }
}

pub struct ShmView<'a, A: CloneFromBackendOwned> {
    inner: ManuallyDrop<
        Box<<A as CloneFromBackendOwned>::BackendOwned, crate::salloc::owner::BackendOwned>,
    >,
    _ctx: ShmRecvContext<'a>,
}

impl<'a, A: CloneFromBackendOwned> ShmView<'a, A> {
    pub(crate) fn new_from_backend_owned(
        backend_owned: Box<
            <A as CloneFromBackendOwned>::BackendOwned,
            crate::salloc::owner::BackendOwned,
        >,
        ctx: ShmRecvContext<'a>,
    ) -> Self {
        ShmView {
            inner: ManuallyDrop::new(backend_owned),
            _ctx: ctx,
        }
    }
}

impl<'a, A: CloneFromBackendOwned> ShmView<'a, A> {
    pub fn into_owned(self) -> A {
        <A as CloneFromBackendOwned>::clone_from_backend_owned(&self.inner)
    }
}

impl<'a, A: CloneFromBackendOwned> Deref for ShmView<'a, A> {
    type Target = <A as CloneFromBackendOwned>::BackendOwned;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, A> Eq for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Eq,
{
}

impl<'a, A> Ord for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Ord,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<'a, 'b, A, B> PartialEq<ShmView<'b, B>> for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned:
        PartialEq<<B as CloneFromBackendOwned>::BackendOwned>,
    B: CloneFromBackendOwned,
{
    #[inline]
    fn eq(&self, other: &ShmView<'b, B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<'a, A> PartialOrd for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: PartialOrd,
{
    #[inline]
    fn partial_cmp(&self, other: &ShmView<A>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<'a, A> fmt::Debug for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a, A> fmt::Display for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, A> Hash for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Hash,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<'a, A: CloneFromBackendOwned> AsRef<<A as CloneFromBackendOwned>::BackendOwned>
    for ShmView<'a, A>
{
    fn as_ref(&self) -> &<A as CloneFromBackendOwned>::BackendOwned {
        &**self
    }
}
