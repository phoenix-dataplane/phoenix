use super::ctrl_msg::ControlMessage;

// contains a buffer for a single control message
// caller must ensure it will not be reused until
// outstading post_send request with respect to it finished
pub struct ControlMsgBuffer {
    buffer: Box<ControlMessage>,
    pub(crate) in_use: bool,
}

impl ControlMsgBuffer {
    pub fn new() -> ControlMsgBuffer {
        let buffer = Box::new_uninit();

        ControlMsgBuffer {
            buffer: unsafe { buffer.assume_init() },
            in_use: false,
        }
    }

    #[inline]
    pub fn as_ctrl_msg_ptr(&mut self) -> *mut ControlMessage {
        &mut *self.buffer as *mut _
    }
}