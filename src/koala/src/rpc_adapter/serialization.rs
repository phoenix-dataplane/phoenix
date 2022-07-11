use std::collections::BTreeMap;
use std::ffi::OsStr;

use interface::rpc::MessageMeta;
use mrpc_marshal::{ExcavateContext, SgList, ShmRecvMr};
use mrpc_marshal::{MarshalError, UnmarshalError};

pub(crate) type MarshalFn = fn(&MessageMeta, usize) -> Result<SgList, MarshalError>;
pub(crate) type UnmarshalFn = fn(
    &MessageMeta,
    &mut ExcavateContext<spin::Mutex<BTreeMap<usize, ShmRecvMr>>>,
) -> Result<(usize, usize), UnmarshalError>;

pub(crate) struct SerializationEngine {
    _library: libloading::Library,
    // NOTE: Symbol here shall not outlive library.
    #[cfg(unix)]
    marshal_fn: libloading::os::unix::Symbol<MarshalFn>,
    #[cfg(windows)]
    marshal_fn: libloading::os::windows::Symbol<MarshalFn>,
    #[cfg(unix)]
    unmarshal_fn: libloading::os::unix::Symbol<UnmarshalFn>,
    #[cfg(windows)]
    unmarshal_fn: libloading::os::windows::Symbol<UnmarshalFn>,
}

impl SerializationEngine {
    pub(crate) fn new<P: AsRef<OsStr>>(lib: P) -> Result<Self, libloading::Error> {
        let library = unsafe { libloading::Library::new(lib) }?;

        let marshal_fn = unsafe {
            let symbol: libloading::Symbol<MarshalFn> = library.get(b"marshal")?;
            symbol.into_raw()
        };

        let unmarshal_fn = unsafe {
            let symbol: libloading::Symbol<UnmarshalFn> = library.get(b"unmarshal")?;
            symbol.into_raw()
        };

        let module = SerializationEngine {
            _library: library,
            marshal_fn,
            unmarshal_fn,
        };
        Ok(module)
    }

    pub(crate) fn marshal(
        &self,
        meta: &MessageMeta,
        addr_backend: usize,
    ) -> Result<SgList, MarshalError> {
        (self.marshal_fn)(meta, addr_backend)
    }

    pub(crate) fn unmarshal(
        &self,
        meta: &MessageMeta,
        ctx: &mut ExcavateContext<spin::Mutex<BTreeMap<usize, ShmRecvMr>>>,
    ) -> Result<(usize, usize), UnmarshalError> {
        (self.unmarshal_fn)(meta, ctx)
    }
}
