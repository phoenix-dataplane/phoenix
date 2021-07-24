#!/usr/bin/env bash

# adapted from ray project

# Cause the script to exit if a single command fails
set -euo pipefail

# this stops git rev-parse from failing if we run this from the .git directory
builtin cd "$(dirname "${BASH_SOURCE:-$0}")"

ROOT="$(git rev-parse --show-toplevel)"
builtin cd "$ROOT" || exit 1

# GIT_LS_EXCLUDES=(
#   ':(exclude)python/ray/cloudpickle/'
# )

# Format all files, and print the diff to stdout for travis.
format_all() {
    echo "$(date)" "clang-format...."
    if command -v clang-format >/dev/null; then
      git ls-files -- '*.cc' '*.h' '*.proto' "${GIT_LS_EXCLUDES[@]}" | xargs clang-format -i
    fi
    echo "$(date)" "done!"
}

# Format files that differ from main branch. Ignores dirs that are not slated
# for autoformat yet.
format_changed() {
    # The `if` guard ensures that the list of filenames is not empty, which
    # could cause yapf to receive 0 positional arguments, making it hang
    # waiting for STDIN.
    #
    # `diff-filter=ACRM` and $MERGEBASE is to ensure we only format files that
    # exist on both branches.
    MERGEBASE="$(git merge-base origin/main HEAD)"

    echo "$(date)" "clang-format...."
    if which clang-format >/dev/null; then
        if ! git diff --diff-filter=ACRM --quiet --exit-code "$MERGEBASE" -- '*.cc' '*.h' &>/dev/null; then
            git diff --name-only --diff-filter=ACRM "$MERGEBASE" -- '*.cc' '*.h' | xargs -P 5 \
                 clang-format -i
        fi
    fi
    echo "$(date)" "done!"
}


# This flag formats individual files. --files *must* be the first command line
# arg to use this option.
if [ "${1-}" == '--all' ]; then
    format_all "${@}"
    git --no-pager diff
else
    # Add the origin remote if it doesn't exist
    if ! git remote -v | grep -q origin; then
        git remote add 'origin' 'git@github.com:koalanet-project/koala.git'
    fi

    # Only fetch master since that's the branch we're diffing against.
    git fetch origin main || true

    # Format only the files that changed in last commit.
    format_changed
fi

if ! git diff --quiet &>/dev/null; then
    echo 'Reformatted changed files. Please review and stage the changes.'
    echo 'Files updated:'
    echo

    git --no-pager diff --name-only

    exit 1
fi
