#!/usr/bin/env bash

cargo rr --bin launcher -- -o /tmp/mrpc-eval --benchmark ./launch_koala.toml --configfile ./config.toml --timeout 600
