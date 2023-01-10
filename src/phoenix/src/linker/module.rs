use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::{Object, SymbolKind};

use mmap::{Mmap, MmapOptions};

use super::initfini::InitFini;
use super::relocation::do_relocation;
use super::section::{CommonSection, ExtraSymbolSection, Section};
use super::symbol::{SymbolLookupTable, SymbolTable};
use super::Error;

pub(crate) struct LoadableModule {
    /// Sections of the module
    sections: Vec<Section>,
    /// Initializers of the ObjectCode
    init: Vec<InitFini>,
    fini: Vec<InitFini>,
    /// Table for symbols within this module
    symtab: SymbolTable,
    /// Memory section for COMMON symbols
    common_section: CommonSection,
    /// The memory map needs to be retained.
    image: Mmap,
    /// The File must be the last to drop.
    object: fs::File,
}

impl LoadableModule {
    /// Load a given object file into memory and resolve undefined symbols.
    pub(crate) fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        // Relocatable object does not have segments, so we have to understand the
        // meaning of each section and load needed sections into memory.
        let object = fs::File::open(&path)?;

        // Map anonymous with RWE
        let image = MmapOptions::new()
            .set_fd(object.as_raw_fd())
            .private(true)
            .read(true)
            .write(true)
            .exec(true)
            .mmap()?;

        let image_start = image.as_ptr();

        // The step to verify ELF is included in `parse`.
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*image)?;

        // Init sections
        let mut sections: Vec<_> = elf.sections().map(|s| Section::new(&s)).collect();

        // Identify initializer and finalizer list
        let init = Vec::new();
        let fini = Vec::new();
        for sec in &sections {
            // let _f = InitFini::new(sec);
        }

        // Update runtime address and allocate space for bss
        for sec in &mut sections {
            sec.update_runtime_addr(image_start)?;
        }

        // Allocate space for SHN_COMMON. See ELF Spec 1-19
        let mut symtab = SymbolTable::new(&elf);
        let mut common_section = CommonSection::new(&symtab)?;

        // Update the symbol to point to the address we allocated for each section
        for sym in &mut symtab.symbols {
            let sym_addr = if sym.is_common {
                Some(common_section.alloc_entry_for_symbol(&sym).addr() as u64)
            } else if sym.is_definition {
                let secno = sym.section_index.expect("You catch an outlier");
                let section = sections.get(secno.0).expect("Invalid ELF section index");
                Some(section.address + sym.address) // base + offset
            } else {
                None
            };

            if let Some(sym_addr) = sym_addr {
                sym.address = sym_addr;
            }
        }

        Ok(Self {
            sections,
            init,
            fini,
            symtab,
            common_section,
            image,
            object,
        })
    }

    /// Insert symbol definition for global symbols from this module into global symbol table
    pub(crate) fn update_global_symbol_table(&self, sym_lookup_table: &mut SymbolLookupTable) {
        for sym in &self.symtab.symbols {
            if sym.is_global && (sym.is_definition || sym.is_common) {
                sym_lookup_table.insert(sym.name.clone(), sym.clone());
            }
        }
    }

    /// Performa relocation
    pub(crate) fn link(
        mut self,
        sym_lookup_table: &SymbolLookupTable,
    ) -> Result<LinkedModule, Error> {
        // Resolve symbols
        //
        // First we resolve section symbols
        // these are special symbols that point to sections and have no name.
        // Usually there should be one symbol for each text or data section.
        //
        // We need to resolve (assign addresses to) them in advance, to be able to use them
        // during the relocation later.
        for sym in self.symtab.symbols.iter_mut() {
            if sym.kind == SymbolKind::Section {
                let secno = sym.section_index.expect("This seems to be an exception");
                sym.address = self.sections[secno.0].address;
            }
        }

        // Allocate space for GOT/PLT sections
        let mut extra_symbol_section = ExtraSymbolSection::new(self.symtab.symbols.len())?;

        // Then we process the reloation sections.
        do_relocation(
            self.image.as_ptr().addr(),
            &self.sections,
            &self.symtab,
            &mut extra_symbol_section,
            &sym_lookup_table,
        );

        Ok(LinkedModule {
            sections: self.sections,
            init: self.init,
            fini: self.fini,
            common_section: self.common_section,
            extra_symbol_section,
            image: self.image,
            object: self.object,
        })
    }
}

pub(crate) struct LinkedModule {
    /// Sections of the module
    sections: Vec<Section>,
    /// Initializers of the ObjectCode
    init: Vec<InitFini>,
    fini: Vec<InitFini>,
    /// Memory section for COMMON symbols
    common_section: CommonSection,
    /// Section to store extra symbols (e.g., for GOT)
    extra_symbol_section: ExtraSymbolSection,
    /// The memory map needs to be retained.
    image: Mmap,
    /// The File must be the last to drop.
    object: fs::File,
}

impl LinkedModule {
    pub(crate) fn run_init(&mut self) {
        eprintln!("TODO: Run initializers");
    }

    pub(crate) fn run_fini(&mut self) {
        eprintln!("TODO: Run finitializers");
    }

    // pub(crate) fn name(&self) -> &str {
    // }

    // pub(crate) fn path(&self) -> &str {
    // }
}
