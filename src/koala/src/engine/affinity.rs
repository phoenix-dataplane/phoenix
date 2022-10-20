use std::fmt;

pub(crate) use libnuma::masks::CpuMask;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreMask(CpuMask);

unsafe impl Send for CoreMask {}

impl CoreMask {
    pub(crate) fn from_numa_node(numa_node_affinity: Option<u8>) -> Self {
        use libnuma::masks::indices::{CpuIndex, NodeIndex};
        use libnuma::masks::Mask;
        let all_cpus = CpuIndex::number_of_permitted_cpus();
        match numa_node_affinity {
            None => {
                // Do not use CpuMask::all()!!!!!
                let cpu_mask = CpuMask::allocate();
                for i in 0..all_cpus {
                    cpu_mask.set(CpuIndex::new(i as _));
                }
                CoreMask(cpu_mask)
            }
            Some(node) => {
                let mut node = NodeIndex::new(node);
                let cpu_mask = node.node_to_cpus();
                CoreMask(cpu_mask)
            }
        }
    }

    pub(crate) fn sched_set_affinity_for_current_thread(&self) -> bool {
        self.0.sched_set_affinity_for_current_thread()
    }

    #[allow(unused)]
    pub(crate) fn is_set(&self, i: u16) -> bool {
        use libnuma::masks::indices::CpuIndex;
        use libnuma::masks::Mask;
        self.0.is_set(CpuIndex::new(i))
    }
}

impl fmt::Display for CoreMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use libnuma::masks::indices::CpuIndex;
        use libnuma::masks::Mask;
        let all_cpus = CpuIndex::number_of_permitted_cpus();
        let mut masks = Vec::with_capacity((all_cpus + 32 - 1) / 32);
        for i in (0..all_cpus).step_by(32) {
            let mut mask = 0;
            let nbits = (all_cpus - i).min(32);
            for j in 0..nbits {
                if self.0.is_set(CpuIndex::new((i + j) as u16)) {
                    mask |= 1 << j;
                }
            }
            if nbits == 32 {
                masks.push(format!("{:08x}", mask));
            } else {
                let mut mask_str = String::new();
                for j in 0..nbits / 4 {
                    mask_str.insert_str(0, &format!("{:0x}", mask >> (j * 4) & 0xf));
                }
                masks.push(mask_str);
            }
        }
        masks.reverse();
        write!(f, "{:?}", masks)
    }
}
