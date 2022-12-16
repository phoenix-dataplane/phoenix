//! A runtime-linker for load a libplugin.rlib into the phoenix's address space.
//! It does the following things.
//!     1. Construct a symbol table
//!         - find out the relocation offset
//!     2. Load the plugin rlib
//!         - parse its dependencies and perform the following steps recursively
//!           for all its dependencies
//!         - extract all objects files from the archive
//!         - ld -r to produce a new single relocatable object file
//!         - to be determinted
//!             - load the object if it does provide new public visible symbols
//!             - perform symbol resolving and relocation
//!         - call the constructor and handle potential errors (i.e., init_module)
//!     3. Update the symbol table to include all new symbols
//!
//! To make sure the linker working, it makes several assumptions. These assumptions
//! may not hold under some special circumstances which requires the user to be
//! extreme careful. Specifically, the linker assumes
//! - all objects are compiled with -fPIC.
//! - there is no LTO for the rlibs (double check whether this condition is necessary)
//! - the compiler toolchain of phoenix itself and the plugins are compatible (the same)
//! - the platform is Linux 64-bit little endian. (This assumption can be removed by
//!   improving the code a little).
use std::alloc::LayoutError;
use std::fs;
use std::io;
use std::path::Path;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::{Object, ObjectSymbol};
use rustc_demangle::demangle;
use thiserror::Error;

pub(crate) mod symbol;
use symbol::{SymbolLookupTable, SymbolTable};

pub(crate) mod section;

pub(crate) mod initfini;

pub(crate) mod module;
use module::LoadableModule;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO: {0}")]
    Io(#[from] io::Error),
    #[error("Object error: {0}")]
    Object(#[from] object::Error),
    #[error("Layout error: {0}")]
    Layout(#[from] LayoutError),
    #[error("Fail to identify relocation offset")]
    RelocationOffset,
}

pub(crate) struct Linker {
    /// The binary for phoenix itself.
    binary: Vec<u8>,
    /// The global symbol lookup table.
    global_sym_table: SymbolLookupTable,
    /// The set of loadable module that are current in memory.
    loaded_modules: Vec<LoadableModule>,
}

impl Linker {
    /// Load the binary of the phoenix itself. Parse the binary headers.
    pub(crate) fn new() -> Result<Self, Error> {
        let self_binary = fs::read("/proc/self/exe")?;
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*self_binary)?;
        println!("entry: {:0x}", elf.entry());

        let Some(runtime_offset) = get_runtime_offset(&elf) else {
            return Err(Error::RelocationOffset);
        };
        println!("runtime_offset: {:0x}", runtime_offset);

        // Update symbols' addresses to their runtime addresses
        let mut global_sym_table = SymbolLookupTable::new(&elf);
        for sym in global_sym_table.table.values_mut() {
            if sym.is_global && sym.is_definition {
                sym.address = (sym.address as isize + runtime_offset) as u64;
            }
        }

        Ok(Linker {
            binary: self_binary,
            global_sym_table,
            loaded_modules: Vec::new(),
        })
    }

    /// Load a given object file into memory.
    pub(crate) fn load_object<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let mut module = LoadableModule::load_and_link(path, &mut self.global_sym_table)?;
        module.run_init();
        Ok(())
    }
}

fn get_runtime_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (get_runtime_offset as *const ()).expose_addr();
    println!("addr of main: {:?}", get_runtime_offset as *const ());

    let target_sym = format!("{}::get_runtime_offset", module_path!());

    let mut num_matches = 0;
    let mut runtime_offset = None;

    for sym in elf.symbols() {
        let demangled_sym = format!("{:#?}", demangle(sym.name().unwrap()));
        if demangled_sym == target_sym {
            let addr = sym.address();
            runtime_offset = Some(runtime_addr as isize - addr as isize);
            num_matches += 1;
        }
    }

    if num_matches > 1 {
        panic!("FIXME, found {} matches for {}", num_matches, target_sym);
    }

    runtime_offset
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_linker() {
        let mut linker = Linker::new().unwrap();
        linker.load_object("/tmp/tmp/core/libcore.o").unwrap();
        linker.load_object("/tmp/tmp/libc/liblibc.o").unwrap();
        linker.load_object("/tmp/tmp/compiler_builtins/libcompiler_builtins-5b83a1df856cf582.o").unwrap();
        linker.load_object("/tmp/tmp/nix/libnix.o").unwrap();
        linker.load_object("/tmp/tmp/libmmap.o").unwrap();
    }
}
