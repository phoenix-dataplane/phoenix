use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use futures::stream::Stream;

struct BenchApp<'a, 'b, 'c, T> {
    // input of the future
    args: &'a Args,
    client: &'b GreeterClient,
    reqs: &'c [RpcMessage<HelloRequest>],
    warmup: bool,
    // states of the future
    response_count: usize,
    reply_futures: FuturesUnordered<BoxFuture<'static, T>>,
    starts: Vec<Instant>,
    latencies: Vec<Duration>,
    scnt: usize,
    rcnt: usize,
}

impl<'a, 'b, 'c, F> BenchApp<'a, 'b, 'c, F> {
    fn new(
        args: &'a Args,
        client: &'b GreeterClient,
        reqs: &'c [RpcMessage<HelloRequest>],
        warmup: bool,
    ) -> Self {
        let mut response_count = 0;
        let mut reply_futures = FuturesUnordered::new();
        let mut starts = Vec::with_capacity(args.total_iters);
        let mut latencies = Vec::with_capacity(args.total_iters);

        let mut scnt = 0;
        let mut rcnt = 0;
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

impl<'a, 'b, 'c, T> Future for BenchApp<'a, 'b, 'c, T> {
    type Output = Result<Vec<(Instant, Duration)>, mrpc::Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while self.rcnt < self.args.total_iters {
            // send request
            if self.scnt < self.args.total_iters {
                self.starts.push(Instant::now());
                let fut = self
                    .client
                    .say_hello(&self.reqs[self.scnt % self.args.provision_count]);
                self.reply_futures.push(Box::pin(fut));
                self.scnt += 1;
            }

            // waitany
            if self.rcnt + self.args.provision_count <= self.scnt {
                // We are good to unwrap because rcnt < scnt
                // let _resp = self.reply_futures.next().await.unwrap()?;
                match Pin::new(&mut self.reply_futures).poll_next(cx) {
                    Poll::Ready(Some(_reply)) => {}
                    Poll::Ready(None) => {
                        panic!("impossible")
                    }
                    Poll::Pending => continue,
                };
                self.latencies
                    .push(self.starts[self.latencies.len()].elapsed());
                self.rcnt += 1;
                if self.warmup {
                    eprintln!("warmup: resp {} received", self.response_count);
                    self.response_count += 1;
                }
            }
        }

        Poll::Pending
    }
}
