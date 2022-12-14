use std::fs;
use std::path::Path;
use std::os::unix::io::AsRawFd;

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::Object;

use mmap::{Mmap, MmapOptions};

use super::initfini::InitFini;
use super::section::Section;
use super::symbol::SymbolTable;
use crate::PhoenixResult;

pub(crate) struct LoadableModule {
    sections: Vec<Section>,
    // Initializers of the ObjectCode
    init: Vec<InitFini>,
    fini: Vec<InitFini>,
    // The File must be the last to drop.
    object: fs::File,
}

impl LoadableModule {
    /// Load a given object file into memory and resolve undefined symbols.
    pub(crate) fn load_and_link<P: AsRef<Path>>(
        path: P,
        sym_table: &mut SymbolTable,
    ) -> PhoenixResult<Self> {
        // Relocatable object does not have segments, so we have to understand the
        // meaning of each section and load needed sections into memory.
        let object = fs::File::open(path)?;

        // Map anan with RWE
        let image = MmapOptions::new()
            .set_fd(object.as_raw_fd())
            .anon(true)
            .private(true)
            .read(true)
            .write(true)
            .exec(true)
            .mmap()?;

        let image_addr = image.as_ptr();

        // The step to verify ELF is included in `parse`.
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*image)?;

        // Init sections
        let mut sections: Vec<_> = elf.sections().map(|s| Section::new(&s)).collect();

        // Identify initializer and finalizer list
        let init = Vec::new();
        let fini = Vec::new();

        // Allocate space for sections
        for sec in &mut sections {
            sec.update_runtime_addr(image_addr)?;
        }

        // Allocate space for SHN_COMMON. See ELF Spec 1-19
        // Copy stuff into this module's object symbol table
        // Traverse the symbol table
        for sym in elf.symbols() {
            // Update the symbol to point to the address we allocated for each section
        }

        // Resolve symbols
        todo!();

        Ok(Self {
            sections,
            init,
            fini,
            object,
        })
    }

    pub(crate) fn run_init(&mut self) {
        todo!();
    }
}
