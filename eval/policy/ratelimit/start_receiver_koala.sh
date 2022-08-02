#!/usr/bin/env bash

KOALA_LOG=info cargo rr --bin koala -- --config ./koala-sender.toml
