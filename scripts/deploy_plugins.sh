#!/usr/bin/env bash
prefix="/tmp/koala"
if [[ $# -ge 1 ]]; then
    prefix=$1
fi

rm -rf $prefix/plugins
mkdir -p $prefix/plugins

install -Dm755 target/release/libkoala_mrpc_plugin.so $prefix/plugins/libkoala_mrpc_plugin.so
install -Dm755 target/release/libkoala_rpc_adapter_plugin.so $prefix/plugins/libkoala_rpc_adapter_plugin.so
install -Dm755 target/release/libkoala_tcp_rpc_adapter_plugin.so $prefix/plugins/libkoala_tcp_rpc_adapter_plugin.so
install -Dm755 target/release/libkoala_transport_rdma_plugin.so $prefix/plugins/libkoala_transport_rdma_plugin.so
install -Dm755 target/release/libkoala_transport_tcp_plugin.so $prefix/plugins/libkoala_transport_tcp_plugin.so
install -Dm755 target/release/libkoala_salloc_plugin.so $prefix/plugins/libkoala_salloc_plugin.so
install -Dm755 target/release/libkoala_null_plugin.so $prefix/plugins/libkoala_null_plugin.so
install -Dm755 target/release/libkoala_ratelimit_plugin.so $prefix/plugins/libkoala_ratelimit_plugin.so
install -Dm755 target/release/libkoala_qos_plugin.so $prefix/plugins/libkoala_qos_plugin.so
install -Dm755 target/release/libkoala_hotel_acl_plugin.so $prefix/plugins/libkoala_hotel_acl_plugin.so
