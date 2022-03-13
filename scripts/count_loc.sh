#!/usr/bin/env bash


echo "miscellaneous code:"
find src/experimental -type f -name "*.rs" | xargs wc -l
echo "rdma bindings:"
find src/rdma -type f -name "*.rs" | xargs wc -l
echo "koala examples:"
find src/koala_examples -type f -name "*.rs" | xargs wc -l

echo "koala total:"
find src/ -type f -name "*.rs" | grep -v "experimental" | grep -v bindings.rs | grep -v koala_examples | xargs wc -l
