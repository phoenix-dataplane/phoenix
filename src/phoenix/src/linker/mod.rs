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
//! - there is no LTO for the rlibs (double check whether this condition is necessary)
//! - the compiler toolchain of phoenix itself and the plugins are compatible (the same)
//! - the platform is Linux 64-bit little endian. (This assumption can be removed by
//!   improving the code a little).
use std::collections::HashMap;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use object::elf;
use object::elf::{FileHeader64, SectionHeader64};
use object::endian::LittleEndian;
use object::read::elf::{ElfFile, ElfSymbol, FileHeader, SectionHeader};
use object::read::SymbolSection;
use object::Relocation;
use object::SectionFlags;
use object::SectionIndex;
use object::SectionKind;
use object::{
    Object, ObjectSection, ObjectSymbol, SymbolFlags, SymbolIndex, SymbolKind, SymbolScope,
};
use rustc_demangle::demangle;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO: {0}")]
    Io(#[from] io::Error),
    #[error("Object error: {0}")]
    Object(#[from] object::Error),
}

#[derive(Debug, Clone)]
struct Symbol {
    index: SymbolIndex,
    name: Option<String>,
    address: u64,
    kind: SymbolKind,
    section: SymbolSection,
    is_undefined: bool,
    is_definition: bool,
    is_common: bool,
    is_weak: bool,
    is_global: bool,
    scope: SymbolScope,
    flags: SymbolFlags<SectionIndex>,
}

#[derive(Debug)]
struct Section {
    index: SectionIndex,
    address: u64,
    size: u64,
    align: u64,
    file_range: Option<(u64, u64)>,
    name: Option<String>,
    segment_name: Option<String>,
    kind: SectionKind,
    flags: SectionFlags,
    relocations: Vec<(u64, Relocation)>,
}

type SymbolTable = HashMap<String, Symbol>;

struct InitFini<'bin> {
    entry: fn(),
    _marker: PhantomData<&'bin ()>,
}

struct Module<'b> {
    sections: Vec<Section>,
    // Initializers of the ObjectCode
    init: Vec<InitFini<'b>>,
    fini: Vec<InitFini<'b>>,
}

pub(crate) struct Linker {
    binary: Vec<u8>,
    // elf: ElfFile<'data, FileHeader64<LittleEndian>>,
    sym_table: SymbolTable,
}

impl Linker {
    /// Load the binary of the phoenix itself. Parse the binary headers.
    pub(crate) fn new() -> Result<Self, Error> {
        let self_binary = fs::read("/proc/self/exe")?;
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*self_binary)?;
        println!("entry: {:0x}", elf.entry());

        let relocation_offset = get_relocation_offset(&elf);
        println!("relocation_offset: {:0x}", relocation_offset.unwrap());

        let sym_table = load_symbol_table(&elf);
        Ok(Linker {
            binary: self_binary,
            sym_table,
        })
    }

    /// Load a given
    pub(crate) fn load_object<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        // Relocatable object does not have segments, so we have to understand the
        // meaning of each section and load them into memory if necessary.
        let object_bin = fs::read(path)?;

        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*object_bin)?;

        // The logic is from ghc/rts/linker/Elf.c
        for section in elf.sections() {
            // Identify initializer and finalizer lists
            match section.kind() {
                SectionKind::Text | SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => {
                    let Ok(name) = section.name() else {
                        log::warn!("name parse error, skip");
                        continue;
                    };
                    // .init -> .ctors.\d+ -> .ctors -> .init_array.\d+ -> .init_array
                    match name {
                        ".init" => {}
                        ".fini" => {}
                        _ if name.starts_with(".ctors") => {}
                        _ if name.starts_with(".dtors") => {}
                        _ if name.starts_with(".init_array") => {}
                        _ if name.starts_with(".fini_array") => {}
                        _ => {
                            continue;
                        }
                    }
                }
                SectionKind::Elf(elf::SHT_INIT_ARRAY) => {}
                SectionKind::Elf(elf::SHT_FINI_ARRAY) => {}
                _ => todo!("handle it"),
            }

            // Load sections
            if section.kind().is_bss() && section.size() > 0 {
            } else if section.kind() != SectionKind::Common && section.size() > 0 {
            }
        }

        // Copy stuff into this module's object symbol table
        todo!("what is SHN_COMMON?");
        // Traverse the symbol table
        for sym in elf.symbols() {
            // Update the symbol to point to the address we allocated for each section
        }

        Ok(())
    }
}

fn get_relocation_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (get_relocation_offset as *const ()).expose_addr();
    println!("addr of main: {:?}", get_relocation_offset as *const ());

    let target_sym = format!("{}::get_relocation_offset", module_path!());

    let mut num_matches = 0;
    let mut relocation_offset = None;

    for sym in elf.symbols() {
        let demangled_sym = format!("{:#?}", demangle(sym.name().unwrap()));
        if demangled_sym == target_sym {
            let addr = sym.address();
            relocation_offset = Some(runtime_addr as isize - addr as isize);
            num_matches += 1;
        }
    }

    if num_matches > 1 {
        panic!("FIXME, found {} matches for {}", num_matches, target_sym);
    }

    relocation_offset
}

fn load_symbol_table(elf: &ElfFile<FileHeader64<LittleEndian>>) -> SymbolTable {
    let mut sym_table = HashMap::new();
    for sym in elf.symbols() {
        match sym.name() {
            Ok(name) => {
                let symbol = Symbol {
                    index: sym.index(),
                    name: sym.name().ok().map(|x| x.to_owned()),
                    address: sym.address(),
                    kind: sym.kind(),
                    section: sym.section(),
                    is_undefined: sym.is_undefined(),
                    is_definition: sym.is_definition(),
                    is_common: sym.is_common(),
                    is_weak: sym.is_weak(),
                    is_global: sym.is_global(),
                    scope: sym.scope(),
                    flags: sym.flags(),
                };
                sym_table
                    .insert(name.to_owned(), symbol)
                    .unwrap_or_else(|| panic!("duplicated symbols: {}", name));
            }
            Err(e) => todo!("The symbol does not have a name, handle the error: {}", e),
        }
    }
    sym_table
}
