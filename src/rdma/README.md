# rdma-rs

```
~/.cargo/bin/bindgen \
	-o bindings.rs \
	--blocklist-type max_align_t \
	--blocklist-type ibv_wc \
	--no-prepend-enum-name \
	--bitfield-enum ibv_access_flags \
	--bitfield-enum ibv_qp_attr_mask \
	--bitfield-enum ibv_wc_flags \
	--bitfield-enum ibv_send_flags \
	--bitfield-enum ibv_port_cap_flags \
	--constified-enum-module ibv_qp_type \
	--constified-enum-module ibv_qp_state \
	--constified-enum-module ibv_port_state \
	--constified-enum-module ibv_wc_opcode \
	--constified-enum-module ibv_wr_opcode \
	--constified-enum-module ibv_wc_status \
	--constified-enum-module rdma_port_space \
	--constified-enum-module rdma_cm_event_type \
	src/rdma_verbs_wrapper.h -- -I/usr/include
```
