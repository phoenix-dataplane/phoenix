use std::env;
use std::io;
use std::path::PathBuf;

use memfd::Memfd;
use thiserror::Error;

use ipc::salloc::{cmd, dp};
use ipc::service::ShmService;

const DEFAULT_KOALA_PREFIX: &str = "/tmp/koala";
const DEFAULT_KOALA_CONTROL: &str = "control.sock";

lazy_static::lazy_static! {
    pub static ref KOALA_PREFIX: PathBuf = {
        env::var("KOALA_PREFIX").map_or_else(|_| PathBuf::from(DEFAULT_KOALA_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{:?} is not a directly", path);
            path
        })
    };

    pub static ref KOALA_CONTROL_SOCK: PathBuf = {
        env::var("KOALA_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_KOALA_CONTROL), PathBuf::from)
    };
}

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SA_CTX: SAContext = SAContext::register().expect("koala salloc register failed");
}

macro_rules! rx_recv_impl {
    ($srv:expr, $resp:path) => {
        match $srv.recv_comp().unwrap().0 {
            Ok($resp) => Ok(()),
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $ok_block:block) => {
        match $srv.recv_comp().unwrap().0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $srv.recv_comp().unwrap().0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $ok_block:block, $err:ident, $err_block:block) => {
        match $srv.recv_comp().unwrap().0 {
            Ok($resp) => $ok_block,
            Err($err) => $err_block,
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
}

pub(crate) struct SAContext {
    pub(crate) service:
        ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl SAContext {
    fn register() -> Result<SAContext, Error> {
        let service = ShmService::register(
            KOALA_PREFIX.as_path(),
            KOALA_CONTROL_SOCK.as_path(),
            "Salloc".to_string(),
        )?;
        Ok(Self { service })
    }
}

fn allocate_shm(len: usize) -> Result<usize, Error> {
    assert!(len > 0);
    SA_CTX.with(|ctx| {
        // TODO(cjr): use a correct align
        let align = len;
        let req = cmd::Command::AllocShm(len, align);
        ctx.service.send_cmd(req)?;
        let fds = ctx.service.recv_fd()?;

        assert_eq!(fds.len(), 1);

        let memfd = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error())?;
        let file_len = memfd.as_file().metadata()?.len() as usize;
        assert!(file_len >= len);

        match ctx.service.recv_comp().unwrap().0 {
            Ok(cmd::CompletionKind::AllocShm(remote_addr, _file_off)) => Ok(remote_addr),
            Err(e) => Err(Error::Interface("AllocShm", e)),
            otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
        }
    })
}

fn dealloc_shm(remote_addr: usize) {
    SA_CTX.with(|ctx| {
        let req = cmd::Command::DeallocShm(remote_addr);
        ctx.service.send_cmd(req).expect("fail to dealloc");
        rx_recv_impl!(ctx.service, cmd::CompletionKind::DeallocShm).expect("fail to dealloc");
    })
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
}

fn main() {
    // 64MB
    let size = 1024 * 1024 * 64;
    loop {
        let start = std::time::Instant::now();
        let addr = allocate_shm(size).unwrap();
        dealloc_shm(addr);
        let elapsed = start.elapsed().as_millis();
        println!("Alloc and dealloc 64MB, latency={}ms", elapsed);
    }
}
