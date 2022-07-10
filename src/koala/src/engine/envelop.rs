use std::borrow::Cow;
use std::alloc::Layout;
use std::any::Any;

use tokio::sync::mpsc::UnboundedSender as CommandSender;
use tokio::sync::mpsc::UnboundedReceiver as CommandReceiver;

pub type TypeTag = Cow<'static, str>;

pub trait TypeTagged {
    fn type_tag_() -> TypeTag
    where
        Self: Sized;

    fn type_tag(&self) -> TypeTag;
    fn type_name(&self) -> Cow<str>;
    fn type_layout(&self) -> Layout;
}

pub trait AnyResource: TypeTagged + Send + 'static {} 

impl dyn AnyMessage {

}

pub trait AnyMessage: Send + 'static {}

pub(crate) struct AnyCommandSender(Box<dyn AnyResource>);

impl AnyCommandSender {
    pub(crate) fn new<T: AnyMessage>(sender: CommandSender<T>) -> Self {
        AnyCommandSender(Box::new(sender))
    }
}

