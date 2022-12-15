use std::collections::HashMap;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::{ElfFile, ElfSymbol};
use object::read::SymbolSection;
use object::{
    Object, ObjectSymbol, ObjectSymbolTable, SectionIndex, SymbolFlags, SymbolIndex, SymbolKind,
    SymbolScope,
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
            match sym.name() {
                Ok(name) => {
                    let symbol = Symbol::new(sym);
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

    pub(crate) fn get(&self, name: &str) -> Option<&Symbol> {
        self.table.get(name)
    }
}
