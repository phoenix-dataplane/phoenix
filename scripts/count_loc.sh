#!/usr/bin/env bash

function info {
	echo "$(tput bold)$(tput setaf 10)"$*"$(tput sgr 0)"
}


info "miscellaneous code:"
find src/experimental -type f -name "*.rs" | xargs wc -l
info "3rdparty slabmalloc:"
find src/slabmalloc -type f -name "*.rs" | xargs wc -l
info "3rdparty prost:"
find src/3rdparty/prost -type f -name "*.rs" | xargs wc -l
info "rdma bindings:"
find src/rdma -type f -name "*.rs" | grep -v bindings | xargs wc -l
info "phoenix examples:"
find src/phoenix_examples -type f -name "*.rs" | xargs wc -l

info "phoenix total:"
find src/ -type f -name "*.rs" | grep -v "experimental" | grep -v slabmalloc | grep -v 3rdparty | grep -v bindings.rs | grep -v phoenix_examples | xargs wc -l


info "doc/comments:"
find src/ -type f -name "*.rs" | grep -v "experimental" | grep -v slabmalloc | grep -v 3rdparty | grep -v bindings.rs | grep -v phoenix_examples | xargs grep -r '[[:space:]]*//' | wc -l
