use libphoenix::verbs::{MemoryRegion, SendFlags, WcStatus};

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = libphoenix::cm::CmId::resolve_route((SERVER_ADDR, SERVER_PORT))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");

    let pre_id = builder.build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut recv_mr: MemoryRegion<u8> =
        pre_id.alloc_msgs(128).expect("Memory registration failed!");

    unsafe {
        pre_id
            .post_recv(&mut recv_mr, .., 0)
            .expect("Post recv failed!");
    }

    let id = pre_id.connect(None).expect("Connect failed!");

    eprintln!("Connection established");

    let send_msg = "Hello phoenix server!";
    let send_mr = {
        let mut mr = id
            .alloc_msgs(send_msg.len())
            .expect("Memory registration failed!");
        mr.copy_from_slice(send_msg.as_bytes());
        mr
    };
    unsafe {
        id.post_send(&send_mr, .., 0, SendFlags::SIGNALED)
            .expect("Post send failed!");
    }

    let wc_send = id.get_send_comp().expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);
    let wc_recv = id.get_recv_comp().expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    println!("{:?}", recv_mr.as_slice());

    assert_eq!(&recv_mr[..send_mr.len()], "Hello phoenix client!".as_bytes());
    Ok(())
}
