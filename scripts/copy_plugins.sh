rm -rf /tmp/koala/plugins
mkdir /tmp/koala/plugins
cp target/release/libkoala_mrpc_plugin.so /tmp/koala/plugins/libkoala_mrpc_plugin.so
cp target/release/libkoala_rpc_adapter_plugin_v1.so /tmp/koala/plugins/libkoala_rpc_adapter_plugin_v1.so
cp target/release/libkoala_rpc_adapter_plugin_v2.so /tmp/koala/plugins/libkoala_rpc_adapter_plugin_v2.so
cp target/release/libkoala_transport_rdma_plugin.so /tmp/koala/plugins/libkoala_transport_rdma_plugin.so
cp target/release/libkoala_salloc_plugin.so /tmp/koala/plugins/libkoala_salloc_plugin.so