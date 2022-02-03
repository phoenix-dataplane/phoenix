use libkoala::verbs::{MemoryRegion, SendFlags, VerbsRequest, ScatterGatherElement, VerbsRequestOpcode};


const SERVER_ADDR: &str = "192.168.211.66";
const SERVER_PORT: u16 = 5000;
const MSG_SIZE: u32 = 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    
    let builder = libkoala::cm::CmId::resolve_route((SERVER_ADDR, SERVER_PORT))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");

    let pre_id = builder.build().expect("Create QP failed!");
    eprintln!("QP created");

    let id = pre_id.connect(None).expect("Connect failed!");
    let mut send_msg = String::new();
    for _ in 0..MSG_SIZE {
        send_msg.push('a');
    }
    let send_mr = {
        let mut mr = id
            .alloc_msgs(MSG_SIZE as usize)
            .expect("Memory registration failed!");
        mr.copy_from_slice(send_msg.as_bytes());
        mr
    };
    eprintln!("Connection established");
    let mut wr_vec = Vec::<VerbsRequest>::new();
    for _ in 0..1 {
        let mut sge_vec = Vec::<ScatterGatherElement>::new();
        sge_vec.push(ScatterGatherElement {
            mr: &send_mr,
            offset: 0, 
            len: MSG_SIZE
        });
        let wr = VerbsRequest {
            wr_id : 0xdeadbeef,
            sg_list: sge_vec,
            sg_num: 1,
            // Each sge consist os addr, length, and MR
            opcode : VerbsRequestOpcode::Send,
            flags : SendFlags::SIGNALED,
            remote_addr : 0,
            remote_key : 0,
            // The second half for UD and Atomic
            imm_data: 0,
            compare_add : 0,
            swap : 0,
            rkey: 0,
            ah: 0,
            remote_qpn : 0,
            remote_qkey: 0,
        };
        wr_vec.push(wr);
    }
    unsafe {
        id.verbs_post_send(wr_vec).expect("bite me");
    }
    Ok(())       
}