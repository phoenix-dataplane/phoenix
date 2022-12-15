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
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolTable {
    pub(crate) table: HashMap<String, Symbol>,
}

impl SymbolTable {
    pub(crate) fn new(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Self {
        let mut sym_table = HashMap::new();
        for sym in elf.symbols() {
            match sym.name() {
                Ok(name) => {
                    let symbol = Symbol {
                        index: sym.index(),
                        name: name.to_owned(),
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
                    };
                    sym_table
                        .insert(name.to_owned(), symbol)
                        .unwrap_or_else(|| panic!("duplicated symbols: {}", name));
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
}
