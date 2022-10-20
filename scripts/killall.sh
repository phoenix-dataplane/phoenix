#!/usr/bin/env bash

pkill -9 mpstat
pkill -9 rpc-bench
pkill -9 envoy
pkill -9 launcher
pkill -9 phoenix
ssh danyang-06 "pkill -9 mpstat; \
                pkill -9 rpc-bench; \
                pkill -9 envoy; \
                pkill -9 launcher; \
                pkill -9 phoenix"
