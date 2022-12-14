use std::collections::HashMap;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::read::SymbolSection;
use object::{
    Object, ObjectSymbol, SectionIndex, SymbolFlags, SymbolIndex, SymbolKind, SymbolScope,
};

#[derive(Debug, Clone)]
pub(crate) struct Symbol {
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

pub(crate) type SymbolTable = HashMap<String, Symbol>;

pub(crate) fn load_symbol_table(elf: &ElfFile<FileHeader64<LittleEndian>>) -> SymbolTable {
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
