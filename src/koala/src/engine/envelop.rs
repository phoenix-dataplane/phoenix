use std::alloc::Layout;
use std::any::{type_name, Any};
use std::borrow::Cow;

use tokio::sync::mpsc::UnboundedReceiver as CommandReceiver;
use tokio::sync::mpsc::UnboundedSender as CommandSender;

pub type TypeTag = Cow<'static, str>;

pub trait TypeTagged {
    fn type_tag_() -> TypeTag
    where
        Self: Sized;

    fn type_tag(&self) -> TypeTag;
    fn type_name(&self) -> Cow<str>;
    fn type_layout(&self) -> Layout;
}

impl<T: Any> TypeTagged for T {
    fn type_tag_() -> TypeTag
    where
        Self: Sized,
    {
        type_name::<T>().into()
    }

    fn type_tag(&self) -> TypeTag {
        type_name::<T>().into()
    }

    fn type_name(&self) -> Cow<str> {
        type_name::<T>().into()
    }

    fn type_layout(&self) -> Layout {
        Layout::for_value(self)
    }
}

pub trait AnyResource: TypeTagged + Send + 'static {}

impl<T: TypeTagged + Send + 'static> AnyResource for T {}

impl dyn AnyResource {
    /// Returns `true` if the inner type is the same as `T`.
    #[inline]
    pub fn is<T: AnyResource>(&self) -> bool {
        // Get TypeTag of the type this function is instantiated with
        let t = <T as TypeTagged>::type_tag_();

        // Get TypeTag of the type in the trait object
        let concrete = self.type_tag();

        // Compare both TypeTags on equality
        t == concrete
    }

    /// Returns some reference to the inner value if it is of type `T`, or
    /// `None` if it isn't.
    #[inline]
    pub fn downcast_ref<T: AnyResource>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_ref_unchecked()) }
        } else {
            None
        }
    }

    /// Returns some mutable reference to the inner value if it is of type `T`, or
    /// `None` if it isn't.
    #[inline]
    pub fn downcast_mut<T: AnyResource>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_mut_unchecked()) }
        } else {
            None
        }
    }

    /// Returns a reference to the inner value as type `dyn T`.
    #[inline]
    pub unsafe fn downcast_ref_unchecked<T: AnyResource>(&self) -> &T {
        debug_assert!(self.is::<T>());
        unsafe { &*(self as *const dyn AnyResource as *const T) }
    }

    /// Returns a mutable reference to the inner value as type `dyn T`.
    #[inline]
    pub unsafe fn downcast_mut_unchecked<T: AnyResource>(&mut self) -> &mut T {
        debug_assert!(self.is::<T>());
        unsafe { &mut *(self as *mut dyn AnyResource as *mut T) }
    }
}

pub trait ResourceDowncast: Sized {
    type Container<T>;

    /// Attempt to downcast AnyResource to a concrete type.
    fn downcast<T: AnyResource>(self) -> Result<Self::Container<T>, Self>;

    /// Downcast AnyResource to a concrete type
    unsafe fn downcast_unchekced<T: AnyResource>(self) -> Self::Container<T>;
}

impl ResourceDowncast for Box<dyn AnyResource> {
    type Container<T> = Box<T>;

    fn downcast<T: AnyResource>(self) -> Result<Self::Container<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchekced()) }
        } else {
            Err(self)
        }
    }

    unsafe fn downcast_unchekced<T: AnyResource>(self) -> Self::Container<T> {
        debug_assert!(self.is::<T>());
        unsafe {
            let raw: *mut dyn AnyResource = Box::into_raw(self);
            Box::from_raw(raw as *mut T)
        }
    }
}

pub trait AnyMessage: Send + 'static {}

pub(crate) struct AnyCommandSender(Box<dyn AnyResource>);

impl AnyCommandSender {
    pub(crate) fn new<T: AnyMessage>(sender: CommandSender<T>) -> Self {
        AnyCommandSender(Box::new(sender))
    }
}
