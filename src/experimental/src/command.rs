use serde::{Deserialize, Serialize};
enum Command {
    Connect(),
}

#[derive(Serialize, Deserialize)]
struct ControlMessage {
    cmd: Command,
}

impl ControlMessage {
    pub fn serialize(msg: ControlMessage) -> Result([u8], Error) {}
}
