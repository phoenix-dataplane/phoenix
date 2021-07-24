#!/usr/bin/env bash

# compare against parent commit
base_commit="HEAD^"
echo "Running clang-format against parent commit $(git rev-parse "$base_commit")"

if command -v git-clang-format; then
	git-clang-format --binary clang-format --commit "$base_commit" --diff
fi
