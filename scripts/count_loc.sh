#!/usr/bin/env bash

function info {
	echo "$(tput bold)$(tput setaf 10)"$*"$(tput sgr 0)"
}


info "miscellaneous code:"
find src/experimental -type f -name "*.rs" | xargs wc -l
info "rdma bindings:"
find src/rdma -type f -name "*.rs" | grep -v bindings | xargs wc -l
info "koala examples:"
find src/koala_examples -type f -name "*.rs" | xargs wc -l

info "koala total:"
find src/ -type f -name "*.rs" | grep -v "experimental" | grep -v bindings.rs | grep -v koala_examples | xargs wc -l
