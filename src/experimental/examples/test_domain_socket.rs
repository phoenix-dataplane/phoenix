use std::fs;
use std::os::unix::io::AsRawFd;

use nix::sys::signal;

use ipc::unix::DomainSocket;

extern "C" fn handle_sigint(sig: i32) {
    log::warn!("sigint catched");
    assert_eq!(sig, signal::SIGINT as i32);

    // clean up
    fs::remove_file(PATH).unwrap();
    fs::remove_file(PATH2).unwrap();
    std::process::exit(0);
}

const PATH: &'static str = "/tmp/server.sock";
const PATH2: &'static str = "/tmp/client.sock";

// test 1: send_fd is non-blocking in certain degree
fn test1() {
    let th1 = std::thread::spawn(|| {
        let mut server = DomainSocket::bind(PATH).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        server.connect(PATH2).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(5));
        server
    });

    let th2 = std::thread::spawn(|| {
        let mut client = DomainSocket::bind(PATH2).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        client.connect(PATH).unwrap();
        eprintln!("send begin");
        client.send_fd(PATH, &[client.as_raw_fd()][..]).unwrap();
        eprintln!("send done");
    });

    th1.join().unwrap();
    th2.join().unwrap();
}

// test 2: recv_fd is non-blocking in certain degree
fn test2() {
    let th1 = std::thread::spawn(|| {
        let mut server = DomainSocket::bind(PATH).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        server.connect(PATH2).unwrap();
        eprintln!("recv begin");
        let fds = server.recv_fd().unwrap();
        eprintln!("recv done");
        server
    });

    let th2 = std::thread::spawn(|| {
        let mut client = DomainSocket::bind(PATH2).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        client.connect(PATH).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(5));
        eprintln!("send begin");
        client.send_fd(PATH, &[client.as_raw_fd()][..]).unwrap();
        eprintln!("send done");
    });

    th1.join().unwrap();
    th2.join().unwrap();
}

fn main() {
    // register sigint handler
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe {
        signal::sigaction(signal::SIGINT, &sig_action).expect("failed to register sighandler");
    }

    // test1();

    test2();
}
