#!/usr/bin/env bash

function usage() {
    echo "Usage: $0 [performance|powersave]"
    exit 0
}

[[ -z "${1-}" ]] && usage "$0"
case $1 in
    performance|powersave)
        echo "Current scaling_governor:"
        cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
        echo "Switching to: $1"
        echo $1 | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor ;;
    *) usage "$0" ;;
esac
