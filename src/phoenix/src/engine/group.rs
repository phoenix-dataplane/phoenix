//! A group of engine that will be packaged together and always
//! share the same scheduling policy.
use std::collections::HashMap;
use std::fmt;

use petgraph::unionfind::UnionFind;

use super::container::EngineContainer;
use super::manager::EngineId;
use super::EngineType;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GroupId(pub(crate) u64);

pub(crate) struct SchedulingGroup {
    /// Group ID.
    pub(crate) id: GroupId,
    /// The engines in this group.
    pub(crate) engines: Vec<(EngineId, EngineContainer)>,
}

impl SchedulingGroup {
    /// Create a new scheduling group from a list of engines
    pub(crate) fn new(gid: GroupId, engines: Vec<(EngineId, EngineContainer)>) -> Self {
        SchedulingGroup { id: gid, engines }
    }
}

impl fmt::Debug for SchedulingGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let engine_names: Vec<String> = self
            .engines
            .iter()
            .map(|e| e.1.engine().description())
            .collect();
        f.debug_struct("SchedulingGroup")
            .field("engines", &engine_names)
            .finish()
    }
}

pub(crate) struct GroupUnionFind {
    index: HashMap<EngineType, usize>,
    union_find: UnionFind<usize>,
}

impl GroupUnionFind {
    pub(crate) fn new(groups: Vec<Vec<EngineType>>) -> Self {
        let mut cnt = 0;
        let mut index = HashMap::new();
        for engine in groups.iter().flatten().copied() {
            index.entry(engine).or_insert_with(|| {
                cnt += 1;
                cnt - 1
            });
        }

        let mut union_find = UnionFind::new(index.len());
        for group in groups.into_iter() {
            for engines in group.windows(2) {
                let ea = *index.get(&engines[0]).unwrap();
                let eb = *index.get(&engines[1]).unwrap();
                union_find.union(ea, eb);
            }
        }
        GroupUnionFind { index, union_find }
    }

    pub(crate) fn is_same_group(&self, a: EngineType, b: EngineType) -> bool {
        let a = match self.index.get(&a) {
            Some(idx) => *idx,
            None => {
                return false;
            }
        };
        let b = match self.index.get(&b) {
            Some(idx) => *idx,
            None => {
                return false;
            }
        };
        self.union_find.equiv(a, b)
    }

    pub(crate) fn find_representative(&self, engine: EngineType) -> Option<usize> {
        let idx = match self.index.get(&engine) {
            Some(idx) => *idx,
            None => return None,
        };
        Some(self.union_find.find(idx))
    }

    #[inline]
    pub(crate) fn size(&self) -> usize {
        self.index.len()
    }
}
