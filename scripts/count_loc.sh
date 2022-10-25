#!/usr/bin/env bash

function info {
	echo "$(tput bold)$(tput setaf 10)"$*"$(tput sgr 0)"
}


info "miscellaneous code:"
find src/experimental -type f -name "*.rs" | xargs wc -l
info "3rdparty slabmalloc:"
find src/slabmalloc -type f -name "*.rs" | xargs wc -l
info "rdma bindings:"
find src/rdma -type f -name "*.rs" | grep -v bindings | xargs wc -l
info "phoenix examples:"
find examples -type f -name "*.rs" | xargs wc -l

info "phoenix total:"
find src/ -type f -name "*.rs" | grep -v experimental | grep -v slabmalloc | grep -v bindings.rs | xargs wc -l
info "phoenix doc/comments:"
find src/ -type f -name "*.rs" | grep -v experimental | grep -v slabmalloc | grep -v bindings.rs | xargs grep -r '[[:space:]]*//' | wc -l

info "3rdparty prost:"
find experimental/mrpc/3rdparty/prost -type f -name "*.rs" | xargs wc -l
info "mrpc examples:"
find experimental/mrpc/examples -type f -name "*.rs" | xargs wc -l
info "mrpc total:"
find experimental/mrpc/ -type f -name "*.rs" | grep -v 3rdparty | grep -v examples | grep -v target | xargs wc -l
info "phoenix doc/comments:"
find experimental/mrpc/ -type f -name "*.rs" | grep -v 3rdparty | grep -v examples | grep -v target | xargs grep -r '[[:space:]]*//' | wc -l

