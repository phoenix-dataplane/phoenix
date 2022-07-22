use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::{RRef, WRef};

pub mod rpc_hello {
    // The string specified here must match the proto package name
    // mrpc::include_proto!("rpc_hello");
    include!("../../../mrpc/src/codegen.rs");
}
use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

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

    #[structopt(long, default_value = "0")]
    pub reply_size: usize,

    #[structopt(long, default_value = "128")]
    pub provision_count: usize,
}

#[derive(Debug)]
struct MyGreeter {
    replies: Vec<WRef<HelloReply>>,
    count: AtomicUsize,
    args: Args,
}

#[mrpc::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello<'s>(
        &self,
        _request: RRef<'s, HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        // eprintln!("reply: {:?}", reply);

        let my_count = self.count.fetch_add(1, Ordering::AcqRel);
        let ret = Ok(WRef::clone(
            &self.replies[my_count % self.args.provision_count],
        ));
        return ret;
    }
}

#[derive(Debug)]
struct MyGreeterBlocking {
    reply: WRef<HelloReply>,
}

#[mrpc::async_trait]
impl Greeter for MyGreeterBlocking {
    async fn say_hello<'s>(
        &self,
        _request: RRef<'s, HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        Ok(WRef::clone(&self.reply))
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    smol::block_on(async {
        let args = Args::from_args();
        let _guard = init_tokio_tracing(&args.log_level, &args.log_dir);

        if args.blocking {
            let message = Vec::new();
            let msg = WRef::new(HelloReply { message });
            let _server = mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
                .add_service(GreeterServer::new(MyGreeterBlocking { reply: msg }))
                .serve()
                .await?;
        } else {
            let mut replies = Vec::new();
            for _ in 0..args.provision_count {
                let mut message = Vec::new();
                message.resize(args.reply_size, 43);
                let msg = WRef::new(HelloReply { message });
                replies.push(msg);
            }
            let _server = mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
                .add_service(GreeterServer::new(MyGreeter {
                    replies,
                    count: AtomicUsize::new(0),
                    args,
                }))
                .serve()
                .await?;
        }

        Ok(())
    })
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
