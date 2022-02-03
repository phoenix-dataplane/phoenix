const SERVER_ADDR: &str = "192.168.211.34";
const SERVER_PORT : u16 = 3000;
fn main() {
    let builder = libkoala::cm::CmId::resolve_route((SERVER_ADDR, SERVER_PORT)).expect("Route Resolve Failed");
    eprintln!("Route resolved");
    let pre_id = builder.build().expect("Create Qp Failed");
    eprintln!("QP created");
}