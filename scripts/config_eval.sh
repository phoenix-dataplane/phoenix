#!/usr/bin/env bash
cd ~/nfs/phoenix
ssh danyang-05 "mkdir -p /tmp/lsh-phoenix; mkdir -p /tmp/mrpc-eval-tcp"
ssh danyang-06 "mkdir -p /tmp/lsh-phoenix; mkdir -p /tmp/mrpc-eval-tcp"
find . -name 'phoenix.toml' | xargs sed -i 's/\/tmp\/phoenix/\/tmp\/lsh-phoenix/g'
find . -name 'config.toml' | xargs sed -i 's/PHOENIX_PREFIX = "\/tmp\/phoenix"/PHOENIX_PREFIX = "\/tmp\/lsh-phoenix"/g'

