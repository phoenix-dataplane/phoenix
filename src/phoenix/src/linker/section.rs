use object::elf;
use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfSection;
use object::{ObjectSection, Relocation, SectionFlags, SectionIndex, SectionKind};

use mmap::{Mmap, MmapOptions};

use super::symbol::{Symbol, SymbolTable};
use super::Error;

#[derive(Debug)]
pub(crate) struct Section {
    pub(crate) index: SectionIndex,
    pub(crate) address: u64,
    pub(crate) size: u64,
    pub(crate) align: u64,
    pub(crate) file_range: Option<(u64, u64)>,
    pub(crate) name: String,
    pub(crate) segment_name: Option<String>,
    pub(crate) kind: SectionKind,
    pub(crate) flags: SectionFlags,
    pub(crate) relocations: Vec<(u64, Relocation)>,
    /// For .bss sections, we need to allocate extra spaces.
    pub(crate) mmap: Option<Mmap>,
}

impl Section {
    pub(crate) fn new(section: &ElfSection<FileHeader64<LittleEndian>>) -> Self {
        Section {
            index: section.index(),
            // We will update the address later.
            address: section.address(),
            size: section.size(),
            align: section.align(),
            file_range: section.file_range(),
            name: section.name().unwrap_or("").to_owned(),
            segment_name: section.segment_name().unwrap_or(None).map(|x| x.to_owned()),
            kind: section.kind(),
            flags: section.flags(),
            relocations: section.relocations().collect::<Vec<(u64, Relocation)>>(),
            // For .bss sections, we need to allocate extra spaces. We'll fill this later.
            mmap: None,
        }
    }

    #[inline]
    pub(crate) fn need_load(&self) -> bool {
        if self.size == 0 {
            return false;
        }

        match self.kind {
            SectionKind::Text
            | SectionKind::Data
            | SectionKind::ReadOnlyData
            | SectionKind::Elf(elf::SHT_INIT_ARRAY)
            | SectionKind::Elf(elf::SHT_FINI_ARRAY) => true,
            _ => false,
        }
    }

    /// Update runtime address for sections needed to load. Allocate memory for .bss sections
    /// if encountered.
    pub(crate) fn update_runtime_addr(&mut self, image_addr: *const u8) -> Result<(), Error> {
        if self.kind.is_bss() && self.size > 0 {
            // Allocate memory for .bss section.
            assert!(self.align as usize <= page_size::get());
            // round up to page
            let rounded_size = self.size.next_multiple_of(page_size::get() as u64) as usize;
            let mmap = MmapOptions::new()
                .len(rounded_size)
                .anon(true)
                .private(true)
                .read(true)
                .write(true)
                .mmap()?;
            // update the address
            self.address = mmap.as_ptr().addr() as u64;
        } else if self.need_load() {
            let file_off = self.file_range.expect("impossible").0;
            self.address = unsafe { image_addr.offset(file_off as isize) }.addr() as u64;
        }
        Ok(())
    }
}

// Common symbols are a feature that allow a programmer to 'define' several
// variables of the same name in different source files.
// This is indeed 'common' in ELF relocatable object files.
pub(crate) struct CommonSection {
    mmap: Mmap,
    used: isize,
}

impl CommonSection {
    pub(crate) fn new(sym_table: &SymbolTable) -> Result<Self, Error> {
        let size: u64 = sym_table
            .table
            .values()
            .filter_map(|sym| if sym.is_common { Some(sym.size) } else { None })
            .sum();
        let mmap = MmapOptions::new()
            .len(size as usize)
            .anon(true)
            .private(true)
            .read(true)
            .write(true)
            .mmap()?;
        Ok(Self { mmap, used: 0 })
    }

    pub(crate) fn alloc_entry_for_symbol(&mut self, sym: &Symbol) -> *const u8 {
        let ret = unsafe { self.mmap.as_ptr().offset(self.used) };
        self.used += sym.size as isize;
        ret
    }
}
