#!/usr/bin/env bash
cd ~/nfs/koala
ssh danyang-05 "mkdir -p /tmp/lsh-koala; mkdir -p /tmp/mrpc-eval-tcp"
ssh danyang-06 "mkdir -p /tmp/lsh-koala; mkdir -p /tmp/mrpc-eval-tcp"
find . -name 'koala.toml' | xargs sed -i 's/\/tmp\/koala/\/tmp\/lsh-koala/g'
find . -name 'config.toml' | xargs sed -i 's/KOALA_PREFIX = "\/tmp\/koala"/KOALA_PREFIX = "\/tmp\/lsh-koala"/g'

