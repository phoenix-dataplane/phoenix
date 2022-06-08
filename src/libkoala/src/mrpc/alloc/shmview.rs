use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::ops::Deref;

use super::boxed::Box;

pub trait CloneFromBackendOwned {
    type BackendOwned;

    fn clone_from_backend_owned(_: &Self::BackendOwned) -> Self;
}

pub struct ShmView<A: CloneFromBackendOwned> {
    inner: ManuallyDrop<
        Box<<A as CloneFromBackendOwned>::BackendOwned, crate::salloc::owner::BackendOwned>,
    >,
}

impl<A: CloneFromBackendOwned> ShmView<A> {
    pub(crate) fn new_from_backend_owned(
        backend_owned: Box<
            <A as CloneFromBackendOwned>::BackendOwned,
            crate::salloc::owner::BackendOwned,
        >,
    ) -> Self {
        ShmView {
            inner: ManuallyDrop::new(backend_owned),
        }
    }
}

impl<A: CloneFromBackendOwned> ShmView<A> {
    pub fn into_owned(self) -> A {
        <A as CloneFromBackendOwned>::clone_from_backend_owned(&self.inner)
    }
}

impl<A: CloneFromBackendOwned> Deref for ShmView<A> {
    type Target = <A as CloneFromBackendOwned>::BackendOwned;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A> Eq for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Eq,
{
}

impl<A> Ord for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Ord,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<A, B> PartialEq<ShmView<B>> for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned:
        PartialEq<<B as CloneFromBackendOwned>::BackendOwned>,
    B: CloneFromBackendOwned,
{
    #[inline]
    fn eq(&self, other: &ShmView<B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<A> PartialOrd for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: PartialOrd,
{
    #[inline]
    fn partial_cmp(&self, other: &ShmView<A>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<A> fmt::Debug for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<A> fmt::Display for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<A> Hash for ShmView<A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Hash,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<A: CloneFromBackendOwned> AsRef<<A as CloneFromBackendOwned>::BackendOwned> for ShmView<A> {
    fn as_ref(&self) -> &<A as CloneFromBackendOwned>::BackendOwned {
        &**self
    }
}
