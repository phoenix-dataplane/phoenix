#!/usr/bin/env bash
prefix="/tmp/phoenix"
if [[ $# -ge 1 ]]; then
    prefix=$1
fi

rm -rf $prefix/plugins
mkdir -p $prefix/plugins

install -Dm755 target/release/libphoenix_mrpc_plugin.so $prefix/plugins/libphoenix_mrpc_plugin.so
install -Dm755 target/release/libphoenix_rpc_adapter_plugin.so $prefix/plugins/libphoenix_rpc_adapter_plugin.so
install -Dm755 target/release/libphoenix_tcp_rpc_adapter_plugin.so $prefix/plugins/libphoenix_tcp_rpc_adapter_plugin.so
install -Dm755 target/release/libphoenix_transport_rdma_plugin.so $prefix/plugins/libphoenix_transport_rdma_plugin.so
install -Dm755 target/release/libphoenix_transport_tcp_plugin.so $prefix/plugins/libphoenix_transport_tcp_plugin.so
install -Dm755 target/release/libphoenix_salloc_plugin.so $prefix/plugins/libphoenix_salloc_plugin.so
install -Dm755 target/release/libphoenix_null_plugin.so $prefix/plugins/libphoenix_null_plugin.so
install -Dm755 target/release/libphoenix_ratelimit_plugin.so $prefix/plugins/libphoenix_ratelimit_plugin.so
install -Dm755 target/release/libphoenix_qos_plugin.so $prefix/plugins/libphoenix_qos_plugin.so
install -Dm755 target/release/libphoenix_hotel_acl_plugin.so $prefix/plugins/libphoenix_hotel_acl_plugin.so
