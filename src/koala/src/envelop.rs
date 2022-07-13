use std::alloc::Layout;
use std::any::{type_name, Any};
use std::borrow::Cow;

use crossbeam::channel::Receiver as DataReceiver;
use crossbeam::channel::Sender as DataSender;
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
    #[inline]
    fn type_tag_() -> TypeTag
    where
        Self: Sized,
    {
        type_name::<T>().into()
    }

    #[inline]
    fn type_tag(&self) -> TypeTag {
        type_name::<T>().into()
    }

    #[inline]
    fn type_name(&self) -> Cow<str> {
        type_name::<T>().into()
    }

    #[inline]
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
        &*(self as *const dyn AnyResource as *const T)
    }

    /// Returns a mutable reference to the inner value as type `dyn T`.
    #[inline]
    pub unsafe fn downcast_mut_unchecked<T: AnyResource>(&mut self) -> &mut T {
        debug_assert!(self.is::<T>());
        &mut *(self as *mut dyn AnyResource as *mut T)
    }
}

pub trait ResourceDowncast: Sized {
    /// Attempt to downcast AnyResource to a concrete type.
    fn downcast<T: AnyResource>(self) -> Result<Box<T>, Self>;

    /// Downcast AnyResource to a concrete type
    unsafe fn downcast_unchecked<T: AnyResource>(self) -> Box<T>;
}

impl ResourceDowncast for Box<dyn AnyResource> {
    #[inline]
    fn downcast<T: AnyResource>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    unsafe fn downcast_unchecked<T: AnyResource>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut dyn AnyResource = Box::into_raw(self);
        Box::from_raw(raw as *mut T)
    }
}

pub trait AnyMessage: Send + 'static {}

pub struct AnyCommandSender(Box<dyn AnyResource>);

impl AnyCommandSender {
    pub fn new<T: AnyMessage>(sender: CommandSender<T>) -> Self {
        AnyCommandSender(Box::new(sender))
    }

    /// Attempt to downcast AnyCommandSender to a concrete type
    #[inline]
    pub fn downcast<T: AnyMessage>(self) -> Result<CommandSender<T>, Self> {
        let sender = self
            .0
            .downcast::<CommandSender<T>>()
            .map_err(|x| AnyCommandSender(x))?;
        Ok(*sender)
    }

    /// Downcast AnyCommandSender to a concrete type
    #[inline]
    pub unsafe fn downcast_unchecked<T: AnyMessage>(self) -> CommandSender<T> {
        let sender = self.0.downcast_unchecked::<CommandSender<T>>();
        *sender
    }

    /// Attempt to downcast AnyCommandSender to a concrete sender and obtain a clone
    /// Command channels should be MPSC, so senders can be cloned, but not receiver
    #[inline]
    pub fn downcast_clone<T: AnyMessage>(&self) -> Option<CommandSender<T>> {
        self.0.downcast_ref::<CommandSender<T>>().map(|x| x.clone())
    }

    /// Downcast AnyCommanderSender and obtain a clone
    pub unsafe fn downcast_clone_unchecked<T: AnyMessage>(&self) -> CommandSender<T> {
        self.0.downcast_ref_unchecked::<CommandSender<T>>().clone()
    }

    /// Returns the type name of the wrapped sender
    #[inline]
    pub fn type_name(&self) -> Cow<str> {
        self.0.type_name()
    }
}

pub struct AnyCommandReceiver(Box<dyn AnyResource>);

pub struct AnyDataSender(Box<dyn AnyResource>);
pub struct AnyDataReceiver(Box<dyn AnyResource>);

macro_rules! channel_impl {
    ($wrapper:ident, $chan:ident) => {
        impl $wrapper {
            pub fn new<T: AnyMessage>(channel: $chan<T>) -> Self {
                $wrapper(Box::new(channel))
            }

            /// Attempt to downcast to a concrete sender/receiver type
            pub fn downcast<T: AnyMessage>(self) -> Result<$chan<T>, Self> {
                let channel = self.0.downcast::<$chan<T>>().map_err(|x| $wrapper(x))?;
                Ok(*channel)
            }

            /// Downcast to a concrete sender/receiver type
            pub unsafe fn downcast_unchecked<T: AnyMessage>(self) -> $chan<T> {
                let channel = self.0.downcast_unchecked::<$chan<T>>();
                *channel
            }

            /// Returns the type name of the wrapped channel
            #[inline]
            pub fn type_name(&self) -> Cow<str> {
                self.0.type_name()
            }
        }
    };
}

channel_impl!(AnyCommandReceiver, CommandReceiver);
channel_impl!(AnyDataSender, DataSender);
channel_impl!(AnyDataReceiver, DataReceiver);
