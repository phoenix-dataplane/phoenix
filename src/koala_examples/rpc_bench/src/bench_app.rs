// use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use futures::stream::Stream;

use mrpc::RRef;

use rpc_hello::HelloReply;

pub(crate) struct BenchApp<'a, 'b, 'c> {
    // input of the future
    args: &'a Args,
    client: &'b GreeterClient,
    reqs: &'c [WRef<HelloRequest>],
    warmup: bool,
    // states of the future
    response_count: usize,
    reply_futures: FuturesUnordered<BoxFuture<'b, Result<RRef<'b, HelloReply>, mrpc::Status>>>,
    starts: Vec<Instant>,
    latencies: Vec<Duration>,
    scnt: usize,
    rcnt: usize,
}

impl<'a, 'b, 'c> BenchApp<'a, 'b, 'c> {
    pub(crate) fn new(
        args: &'a Args,
        client: &'b GreeterClient,
        reqs: &'c [WRef<HelloRequest>],
        warmup: bool,
    ) -> Self {
        let response_count = 0;
        let reply_futures = FuturesUnordered::new();
        let starts = Vec::with_capacity(args.total_iters);
        let latencies = Vec::with_capacity(args.total_iters);

        let scnt = 0;
        let rcnt = 0;
        BenchApp {
            args,
            client,
            reqs,
            warmup,
            // states
            response_count,
            reply_futures,
            starts,
            latencies,
            scnt,
            rcnt,
        }
    }
}

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

impl<'a, 'b, 'c> Future for BenchApp<'a, 'b, 'c>
where
    'b: 'a,
    'b: 'c,
    'c: 'b,
{
    type Output = Result<Vec<(Instant, Duration)>, mrpc::Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if this.rcnt < this.args.total_iters {
            // issue a request
            if this.scnt < this.args.total_iters && this.scnt < this.rcnt + this.args.concurrency {
                this.starts.push(Instant::now());
                let fut = this
                    .client
                    .say_hello(&this.reqs[this.scnt % this.args.provision_count]);
                this.reply_futures.push(Box::pin(fut));
                this.scnt += 1;
            }

            // try_waitany
            // We are good to unwrap because rcnt < scnt (this.args.concurrency > 0)
            // let _resp = this.reply_futures.next().await.unwrap()?;
            match Pin::new(&mut this.reply_futures).poll_next(cx) {
                Poll::Ready(Some(_reply)) => {}
                Poll::Ready(None) => {
                    panic!("impossible")
                }
                Poll::Pending => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            };

            // received a response
            this.latencies
                .push(this.starts[this.latencies.len()].elapsed());
            this.rcnt += 1;
            if this.warmup {
                eprintln!("warmup: resp {} received", this.response_count);
                this.response_count += 1;
            }

            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(std::iter::zip(
                this.starts.clone(),
                this.latencies.clone(),
            )
            .collect()))
        }
    }
}
