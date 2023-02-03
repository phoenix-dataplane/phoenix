use std::collections::HashMap;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::{ElfFile, ElfSymbol};
use object::read::SymbolSection;
use object::{
    Object, ObjectSymbol, ObjectSymbolTable, SectionIndex, SymbolFlags, SymbolIndex, SymbolKind,
    SymbolScope,
};

use super::tls::{phoenix_tls_get_addr, TlsIndex, PHOENIX_MOD_INVALID};

#[derive(Debug, Clone)]
pub(crate) struct Symbol {
    pub(crate) index: SymbolIndex,
    pub(crate) name: String,
    pub(crate) address: u64,
    pub(crate) size: u64,
    pub(crate) kind: SymbolKind,
    pub(crate) section: SymbolSection,
    pub(crate) is_undefined: bool,
    pub(crate) is_definition: bool,
    pub(crate) is_common: bool,
    pub(crate) is_weak: bool,
    pub(crate) is_global: bool,
    pub(crate) scope: SymbolScope,
    pub(crate) flags: SymbolFlags<SectionIndex>,
    pub(crate) section_index: Option<SectionIndex>,
    // Only valid for TLVs, refering to the module defines this TLS variable.
    pub(crate) mod_id: usize,
}

impl Symbol {
    pub(crate) fn new(sym: ElfSymbol<FileHeader64<LittleEndian>>) -> Self {
        Symbol {
            index: sym.index(),
            name: sym.name().expect("Symbol name invalid UTF-8").to_owned(),
            address: sym.address(),
            size: sym.size(),
            kind: sym.kind(),
            section: sym.section(),
            is_undefined: sym.is_undefined(),
            is_definition: sym.is_definition(),
            is_common: sym.is_common(),
            is_weak: sym.is_weak(),
            is_global: sym.is_global(),
            scope: sym.scope(),
            flags: sym.flags(),
            section_index: sym.section_index(),
            mod_id: PHOENIX_MOD_INVALID,
        }
    }
}

/// An owned clone of the original symbol table. Supporting getting symbol by index.
#[derive(Debug, Clone)]
pub(crate) struct SymbolTable {
    pub(crate) symbols: Vec<Symbol>,
}

impl SymbolTable {
    pub(crate) fn new(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Self {
        let symbols = if let Some(symtab) = elf.symbol_table() {
            // NOTE(cjr): This assumes symbols() iterator returns items in order.
            symtab.symbols().map(|s| Symbol::new(s)).collect()
        } else {
            Vec::new()
        };
        Self { symbols }
    }

    pub(crate) fn symbol_by_index(&self, sym_index: SymbolIndex) -> Option<&Symbol> {
        self.symbols.get(sym_index.0)
    }
}

/// Global symbol lookup table. Allowing getting symbol by its name.
#[derive(Debug, Clone)]
pub(crate) struct SymbolLookupTable {
    pub(crate) table: HashMap<String, Symbol>,
}

impl SymbolLookupTable {
    pub(crate) fn new(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Self {
        let mut sym_table = HashMap::new();
        for sym in elf.symbols() {
            if sym.is_undefined() || sym.is_local() {
                continue;
            }
            if sym.kind() == SymbolKind::Unknown {
                continue;
            }
            match sym.name() {
                Ok(name) => {
                    let symbol = Symbol::new(sym);
                    // eprintln!("name: '{}'", name);
                    let ret = sym_table.insert(name.to_owned(), symbol);
                    if ret.is_some() {
                        panic!("duplicate symbol: {:?}", Symbol::new(sym));
                    }
                }
                Err(e) => todo!("The symbol does not have a name, handle the error: {}", e),
            }
        }
        Self { table: sym_table }
    }

    pub(crate) fn insert(&mut self, name: String, sym: Symbol) {
        // TODO(cjr): Do more check for duplicated symbols.
        self.table.insert(name, sym);
    }

    pub(crate) fn lookup_tls_symbol(&self, name: &str) -> Option<TlsIndex> {
        if let Some(addr) = Self::lookup_symbol_dlsym(name) {
            // the symbol is defined in the init binary
            // we can reverse looking up the mod_id and offset in the current thread
            // YES, in any thread.
            //
            // This is because mod_id is globally unified across threads, and offset
            // if just the address of the variable relative to the base tls_data of each
            // thread. It should also be identical across threads!

            unsafe extern "C" fn cb(
                info: *mut libc::dl_phdr_info,
                size: libc::size_t,
                data: *mut libc::c_void,
            ) -> libc::c_int {
                // void* addr = data;
                // if (info->dlpi_tls_data) {
                //   size_t length = 0;
                //   for (int i = 0; i < info->dlpi_phnum; i++) {
                //     if (info->dlpi_phdr[i].p_type == PT_TLS) {
                //       length = info->dlpi_phdr[i].p_memsz;
                //       break;
                //     }
                //   }
                //   printf("start: %p, length: %ld\n", info->dlpi_tls_data, length);
                //   if (data >= info->dlpi_tls_data && data < (char*)info->dlpi_tls_data + length) {
                //     // printf("found!, name: %s\n", info->dlpi_name);
                //     printf("found! %s, %ld, tls_data: %p\n", info->dlpi_name, info->dlpi_tls_modid, info->dlpi_tls_data);
                //   }
                // }
                assert!(
                    size >= 32,
                    "info does not contain extension fields (dlpi_tls_modid and dlpi_tls_data)"
                );
                let input_output: &mut [usize; 4] = unsafe { &mut *(data as *mut [usize; 4]) };
                let addr = input_output[0];
                let info: &libc::dl_phdr_info = unsafe { &*info };
                if !info.dlpi_tls_data.is_null() {
                    let mut length = 0;
                    for i in 0..info.dlpi_phnum {
                        let phdr = unsafe { &*info.dlpi_phdr.add(i as usize) };
                        if phdr.p_type == libc::PT_TLS {
                            length = phdr.p_memsz as usize;
                            break;
                        }
                    }
                    let start = info.dlpi_tls_data.expose_addr();
                    if addr >= start && addr < start + length {
                        input_output[1] = info.dlpi_tls_modid;
                        input_output[2] = addr - start;
                        input_output[3] = 1; // found
                    }
                }
                0
            }

            // addr to query, returns mod_id, offset
            let mut input_output: [usize; 4] = [addr, 0, 0, 0];
            unsafe {
                libc::dl_iterate_phdr(Some(cb), &mut input_output as *mut _ as *mut libc::c_void)
            };
            if input_output[3] == 1 {
                // found
                Some(TlsIndex::new(input_output[1], input_output[2]))
            } else {
                panic!("reverse mod_id and offset for {} failed", name);
            }
        } else {
            match self.table.get(name) {
                Some(sym) => {
                    assert!(!sym.is_undefined, "sym: {:?}", sym);
                    Some(TlsIndex::new(sym.mod_id, sym.address as usize))
                }
                None => None,
            }
        }
    }

    pub(crate) fn lookup_symbol_addr(&self, name: &str) -> Option<usize> {
        match name {
            "__tls_get_addr" => {
                return Some((phoenix_tls_get_addr as *const ()).addr());
            }
            "__rust_probestack" => {
                return Some((__rust_probestack as *const ()).addr());
            }
            _ => {}
        }

        if let Some(addr) = Self::lookup_symbol_dlsym(name) {
            // if name == "_ZN3std11collections4hash3map11RandomState3new4KEYS7__getit5__KEY17h32461f6f947bc20aE" {
            //     panic!("here, sym.address: {:0x}", addr);
            // }
            Some(addr)
        } else {
            match self.table.get(name) {
                Some(sym) => {
                    // if name == "_ZN3std11collections4hash3map11RandomState3new4KEYS7__getit5__KEY17h32461f6f947bc20aE" {
                    //     panic!("here, sym.address: {:0x}", sym.address);
                    // }
                    if name == "init_module_salloc" {
                        eprintln!("here, sym.address: {:0x}", sym.address);
                    }
                    Some(sym.address as usize)
                }
                None => None,
            }
        }
    }

    pub(crate) fn lookup_symbol_dlsym(name: &str) -> Option<usize> {
        // In case we did not find the symbol in the global defined symbols,
        // we try to look up the symbol using dlsym.
        let cstr = std::ffi::CString::new(name).expect("Invalid name for CString");
        let addr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, cstr.as_c_str().as_ptr()) };
        // TODO(cjr): look up in opened shared libraries.
        if addr.is_null() {
            // eprintln!("{:?}", unsafe { std::ffi::CStr::from_ptr(libc::dlerror()) });
            None
        } else {
            Some(addr.addr())
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExtraSymbol {
    pub(crate) addr: usize,
    pub(crate) trampoline: [u8; 8],
}

// special symbols that dlsym cannot find
extern "C" {
    pub fn __rust_probestack();
}
