#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

python3 attach_policy.py ${OD}

sleep 1

PWD=$(pwd)
ssh danyang-06 "cd ${PWD}; python3 attach_policy.py ${OD}"
