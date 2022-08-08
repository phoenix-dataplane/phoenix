#!/usr/bin/env bash

KOALA_LOG=info numactl -N 0 -m 0 cargo rr --bin koala -- --config ./koala-sender.toml
