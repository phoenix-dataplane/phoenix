#![feature(allocator_api)]
use std::path::PathBuf;

use structopt::StructOpt;

use libkoala::mrpc::alloc::{ShmView, Vec};
use libkoala::mrpc::codegen::{Greeter, GreeterServer, HelloReply, HelloRequest};
use libkoala::mrpc::stub::RpcMessage;

#[derive(StructOpt, Debug)]
#[structopt(about = "Koala RPC hello client")]
pub struct Args {
    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Blocking or not?
    #[structopt(short = "b", long)]
    pub blocking: bool,

    #[structopt(short = "l", long, default_value = "error")]
    pub log_level: String,

    #[structopt(long)]
    pub log_dir: Option<PathBuf>,
}

// TODO(wyj): add back debug
// #[derive(Debug)]
struct MyGreeter {
    replies: Vec<RpcMessage<HelloReply>>,
    count: usize,
}

impl Greeter for MyGreeter {
    fn say_hello(
        &mut self,
        _request: ShmView<HelloRequest>,
    ) -> Result<&mut RpcMessage<HelloReply>, libkoala::mrpc::Status> {
        // eprintln!("reply: {:?}", reply);

        let ret = Ok(&mut self.replies[self.count]);
        self.count = (self.count + 1) % 128;
        return ret;
    }
}

// TODO(wyj): add back debug
// #[derive(Debug)]
struct MyGreeterBlocking {
    reply: RpcMessage<HelloReply>,
}

impl Greeter for MyGreeterBlocking {
    fn say_hello(
        &mut self,
        _request: ShmView<HelloRequest>,
    ) -> Result<&mut RpcMessage<HelloReply>, libkoala::mrpc::Status> {
        Ok(&mut self.reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let args = Args::from_args();
    let _guard = init_tokio_tracing(&args.log_level, &args.log_dir);

    if args.blocking {
        let name = Vec::new();
        let msg = RpcMessage::new(HelloReply { name });
        let _server = libkoala::mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
            .add_service(GreeterServer::new(MyGreeterBlocking { reply: msg }))
            .serve()?;
    } else {
        let mut replies = Vec::new();
        for _ in 0..128 {
            let name = Vec::new();
            let msg = RpcMessage::new(HelloReply { name });
            replies.push(msg);
        }
        let _server = libkoala::mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
            .add_service(GreeterServer::new(MyGreeter { replies, count: 0 }))
            .serve()?;
    }

    Ok(())
}

fn init_tokio_tracing(
    level: &str,
    log_directory: &Option<PathBuf>,
) -> tracing_appender::non_blocking::WorkerGuard {
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    let env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .parse(level)
        .expect("invalid tracing level");

    let (non_blocking, appender_guard) = if let Some(log_dir) = log_directory {
        let file_appender = tracing_appender::rolling::minutely(log_dir, "rpc-server.log");
        tracing_appender::non_blocking(file_appender)
    } else {
        tracing_appender::non_blocking(std::io::stdout())
    };

    tracing_subscriber::fmt::fmt()
        .event_format(format)
        .with_writer(non_blocking)
        .with_env_filter(env_filter)
        .init();

    tracing::info!("tokio_tracing initialized");

    appender_guard
}
