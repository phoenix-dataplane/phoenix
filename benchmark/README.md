# Phoenix benchmark suites

There is a set of defined benchmark configurations under `benchmark/`.
To use this launcher, you currently need to first edit `config.toml`,
and run the launcher within this directory (the same directory as this
README). You need to at least update the `workdir` to point the project's
path on your file system. To run Phoenix examples, the `workdir` should 
be set to the path to phoenix project. To run mRPC examples, the `workdir`
must be set to points to the path of `phoenix/experimental/mrpc`.
You also need to have ssh connections to the worker machines
specified in the benchmark configurations.

```
$ cargo rr --bin launcher -- --help
    Finished release [optimized + debuginfo] target(s) in 0.41s
     Running `target/release/launcher --help`
[2023-03-02 11:46:08.073007 INFO benchmark/src/main.rs:632] env_logger initialized
launcher 0.1.0
Launcher of the benchmark suite.

USAGE:
    launcher [FLAGS] [OPTIONS] --benchmark <benchmark>

FLAGS:
        --debug          Run with debug mode (cargo build without --release)
        --dry-run        Dry-run. Use this option to check the configs
    -h, --help           Prints help information
        --logical-and    kill all threads if any thread ends
    -s, --silent         Do out print to stdout
    -V, --version        Prints version information

OPTIONS:
    -b, --benchmark <benchmark>            Run a single benchmark task
    -c, --configfile <configfile>          configfile [default: config.toml]
        --timeout <global-timeout-secs>    Timeout in seconds, 0 means infinity. Can be overwritten by specific case
                                           configs [default: 60]
    -g, --group <group>                    Run a benchmark group
    -o, --output-dir <output-dir>          Output directory of log files
```

To start with the basic connectivity, first update `benchmark/rpc_hello.toml`. Then run (make sure `workdir` is correct)
```
$ cargo rr --bin launcher -- --benchmark benchmark/rpc_hello.toml
```

To test the latency for mRPC, you can run (make sure `workdir` is
correct).
```
$ cargo rr --bin launcher -- -o /tmp/output --benchmark benchmark/rpc_bench_latency/rpc_bench_latency_64b.toml
```

Similarly, to test the bandwidth or RPC rate, you can run
```
$ cargo rr --bin launcher -- -o /tmp/output --benchmark benchmark/rpc_bench_tput/rpc_bench_tput_1mb.toml
or
$ cargo rr --bin launcher -- -o /tmp/output --benchmark benchmark/rpc_bench_rate/rpc_bench_tput_32b_4c.toml
```

You can also specify a __benchmark group__ and run a group of tests.
For more information, please read the commandline usage and the
benchmark configuration files.
