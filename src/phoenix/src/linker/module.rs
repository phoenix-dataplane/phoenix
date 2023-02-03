use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::{Object, SymbolKind};

use mmap::{Mmap, MmapOptions};

use super::initfini::InitFini;
use super::relocation::do_relocation;
use super::section::{CommonSection, ExtraSymbolSection, Section};
use super::symbol::{SymbolLookupTable, SymbolTable};
use super::tls::{TlsInitImage, PHOENIX_MOD_BASE};
use super::Error;

static MODULE_COUNTER: AtomicUsize = AtomicUsize::new(PHOENIX_MOD_BASE);

lazy_static::lazy_static! {
    static ref FILE: std::sync::Mutex<std::fs::File> = std::sync::Mutex::new(std::fs::File::create("/tmp/symbols.txt").unwrap());
}

pub(crate) struct LoadableModule {
    /// mod_id
    mod_id: usize,
    /// Sections of the module
    sections: Vec<Section>,
    /// Initializers of the ObjectCode
    init: Vec<InitFini>,
    fini: Vec<InitFini>,
    /// Table for symbols within this module
    symtab: SymbolTable,
    /// Memory section for COMMON symbols
    common_section: CommonSection,
    /// TLS initialization image
    tls_initimage: TlsInitImage,
    /// The memory map needs to be retained.
    image: Mmap,
    /// Path to the binary
    path: PathBuf,
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

        // Create the TLS initialization image
        let tls_initimage = TlsInitImage::new(&mut sections)?;

        // Allocate space for SHN_COMMON. See ELF Spec 1-19
        let mut symtab = SymbolTable::new(&elf);
        let mut common_section = CommonSection::new(&symtab)?;

        let mod_id = MODULE_COUNTER.fetch_add(1, Ordering::AcqRel);

        let mut file = FILE.lock().unwrap();

        // Update the symbol to point to the address we allocated for each section
        for sym in &mut symtab.symbols {
            let sym_addr = if sym.is_common {
                assert_ne!(sym.kind, SymbolKind::Tls);
                Some(common_section.alloc_entry_for_symbol(&sym).addr() as u64)
            } else if sym.is_definition {
                let secno = sym.section_index.expect("You catch an outlier");
                let section = sections.get(secno.0).expect("Invalid ELF section index");
                Some(section.address + sym.address) // base + offset
            } else if sym.kind == SymbolKind::Tls && !sym.is_undefined {
                let secno = sym.section_index.expect("You catch an outlier");
                let section = sections
                    .get_mut(secno.0)
                    .expect("Invalid ELF section index");
                sym.mod_id = mod_id;
                Some(section.alloc_tlv(&sym))
            } else {
                None
            };

            if let Some(sym_addr) = sym_addr {
                sym.address = sym_addr;
            }

            use std::io::Write;
            writeln!(file, "{}: 0x{:0x}", sym.name, sym.address).unwrap();

            // if sym.name == "_ZN14phoenix_salloc7my_tls27__getit5__KEY17h0a62b7d86b328016E" {
            //     panic!("sym: {:?}, tls_initimage: {:?}", sym, tls_initimage);
            // }
        }

        Ok(Self {
            mod_id,
            sections,
            init,
            fini,
            symtab,
            common_section,
            tls_initimage,
            image,
            path: path.as_ref().to_path_buf(),
            object,
        })
    }

    /// Insert symbol definition for global symbols from this module into global symbol table
    pub(crate) fn update_global_symbol_table(&self, sym_lookup_table: &mut SymbolLookupTable) {
        for sym in &self.symtab.symbols {
            if sym.is_global
                && (sym.is_definition
                    || sym.is_common
                    || (sym.kind == SymbolKind::Tls && !sym.is_undefined))
            {
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
        // We need to resolve (assign addresses to) them in advance, so that they can be used
        // during the later relocation.
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
            mod_id: self.mod_id,
            sections: self.sections,
            init: self.init,
            fini: self.fini,
            common_section: self.common_section,
            extra_symbol_section,
            tls_initimage: self.tls_initimage,
            image: self.image,
            path: self.path,
            object: self.object,
        })
    }
}

pub(crate) struct LinkedModule {
    /// mod_id
    mod_id: usize,
    /// Sections of the module
    sections: Vec<Section>,
    /// Initializers of the ObjectCode
    init: Vec<InitFini>,
    fini: Vec<InitFini>,
    /// Memory section for COMMON symbols
    common_section: CommonSection,
    /// Section to store extra symbols (e.g., for GOT)
    extra_symbol_section: ExtraSymbolSection,
    /// TLS initialization image
    tls_initimage: TlsInitImage,
    /// The memory map needs to be retained.
    image: Mmap,
    /// Path to the binary
    path: PathBuf,
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

    #[inline]
    pub(crate) fn mod_id(&self) -> usize {
        self.mod_id
    }

    #[inline]
    pub(crate) fn tls_initimage(&self) -> &TlsInitImage {
        &self.tls_initimage
    }

    #[inline]
    pub(crate) fn path(&self) -> &PathBuf {
        &self.path
    }

    // pub(crate) fn name(&self) -> &str {
    // }

    // pub(crate) fn path(&self) -> &str {
    // }
}
