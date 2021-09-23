#!/bin/sh

find . | grep "\.rs$" | xargs touch ; cargo clippy
