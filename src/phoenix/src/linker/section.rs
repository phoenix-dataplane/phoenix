use std::ptr;

use object::elf;
use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfSection;
use object::read::{SymbolIndex, SymbolSection};
use object::{ObjectSection, Relocation, SectionFlags, SectionIndex, SectionKind};
use object::{RelocationKind, RelocationTarget};

use mmap::{Mmap, MmapOptions};

use super::symbol::{ExtraSymbol, Symbol, SymbolLookupTable, SymbolTable};
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
            let rounded_size = self
                .size
                .next_multiple_of(self.align)
                .next_multiple_of(page_size::get() as u64) as usize;
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
    mmap: Option<Mmap>,
    used: isize,
}

impl CommonSection {
    pub(crate) fn new(sym_table: &SymbolTable) -> Result<Self, Error> {
        let size: u64 = sym_table
            .symbols
            .iter()
            .filter_map(|sym| if sym.is_common { Some(sym.size) } else { None })
            .sum();
        let size = (size as usize).next_multiple_of(page_size::get());
        let mmap = if size > 0 {
            Some(
                MmapOptions::new()
                    .len(size)
                    .anon(true)
                    .private(true)
                    .read(true)
                    .write(true)
                    .mmap()?,
            )
        } else {
            None
        };
        Ok(Self { mmap, used: 0 })
    }

    pub(crate) fn alloc_entry_for_symbol(&mut self, sym: &Symbol) -> *const u8 {
        let mmap = self
            .mmap
            .as_ref()
            .expect("Something is wrong with calculating common size");
        let ret = unsafe { mmap.as_ptr().offset(self.used) };
        self.used += sym.size as isize;
        assert!(self.used as usize <= mmap.len());
        ret
    }
}

pub(crate) struct ExtraSymbolSection {
    mmap: Option<Mmap>,
    used: isize,
}

impl ExtraSymbolSection {
    pub(crate) fn new(sym_table: &SymbolTable) -> Result<Self, Error> {
        let num_symbols = sym_table.symbols.len();
        let size = num_symbols * std::mem::size_of::<ExtraSymbol>();
        let size = size.next_multiple_of(page_size::get());
        let mmap = if size > 0 {
            let mut mmap = MmapOptions::new()
                .len(size)
                .anon(true)
                .private(true)
                .read(true)
                .write(true)
                .exec(true) /* we have trampoline code in this section */
                .mmap()?;
            // zero-fill
            mmap.fill(0);
            Some(mmap)
        } else {
            None
        };
        Ok(Self { mmap, used: 0 })
    }

    #[inline]
    pub(crate) fn get_base_address(&self) -> usize {
        self.mmap.as_ref().unwrap().as_ptr().addr()
    }

    #[inline]
    pub(crate) fn make_got_entry(&self, sym_addr: usize, sym_index: SymbolIndex) -> usize {
        let start = self
            .mmap
            .as_ref()
            .unwrap()
            .as_ptr()
            .cast::<ExtraSymbol>()
            .cast_mut();
        let entry = &mut unsafe { *start.offset(sym_index.0 as isize) };
        *entry = ExtraSymbol {
            addr: sym_addr,
            trampoline: [0xFF, 0x25, 0xF2, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        };
        ptr::addr_of!(entry.addr).addr()
    }

    #[inline]
    pub(crate) fn make_plt_entry(&self, sym_addr: usize, sym_index: SymbolIndex) -> usize {
        let start = self
            .mmap
            .as_ref()
            .unwrap()
            .as_ptr()
            .cast::<ExtraSymbol>()
            .cast_mut();
        let entry = &mut unsafe { *start.offset(sym_index.0 as isize) };
        *entry = ExtraSymbol {
            addr: sym_addr,
            trampoline: [0xFF, 0x25, 0xF2, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        };
        ptr::addr_of!(entry.addr).addr()
    }
}

#[allow(non_snake_case)]
pub(crate) fn do_relocation(
    image_addr: usize,
    sections: &Vec<Section>,
    local_sym_table: &SymbolTable,
    extra_symbols: &mut ExtraSymbolSection,
    global_sym_table: &SymbolLookupTable,
) {
    for sec in sections {
        if !sec.need_load() {
            continue;
        }

        for (off, rela) in &sec.relocations {
            let mut cur_sym_index = None;
            let P = sec.address + off;
            let A = rela.addend();
            let S = match rela.target() {
                RelocationTarget::Symbol(sym_index) => {
                    cur_sym_index = Some(sym_index);
                    let sym = local_sym_table.symbol_by_index(sym_index).unwrap();
                    if sym.is_global {
                        // for global symbols, get its name first
                        // then query the symbol in the global symbol lookup table
                        // eprintln!("name: {}, rela.kind: {:?}", sym.name, rela.kind());
                        let addr = global_sym_table
                            .lookup_symbol_addr(&sym.name)
                            .unwrap_or_else(|| panic!("missing symbol {}", sym.name));
                        addr as u64
                    } else {
                        // for local symbols, just read its symbol address
                        let SymbolSection::Section(section_index) = sym.section else {
                            panic!("no such section: {:?}", sym.section);
                        };
                        let section = &sections[section_index.0];
                        section.address + sym.address
                    }
                }
                RelocationTarget::Section(_sec_index) => todo!("Got a section to relocate"),
                RelocationTarget::Absolute => 0,
                _ => panic!("rela: {:?}", rela),
            };

            let (P, A, S) = (P as i64, A as i64, S as i64);
            let Image = image_addr as i64;
            let Section = sec.address as i64;
            let GotBase = extra_symbols.get_base_address() as i64;
            let value = match rela.kind() {
                RelocationKind::Absolute => S + A,
                RelocationKind::Relative => S + A - P,
                RelocationKind::Got => {
                    let G = extra_symbols
                        .make_got_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    G + A - GotBase
                }
                RelocationKind::GotRelative => {
                    let G = extra_symbols
                        .make_got_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    G + A - P
                }
                RelocationKind::GotBaseRelative => GotBase + A - P,
                RelocationKind::GotBaseOffset => S + A - GotBase,
                RelocationKind::PltRelative => {
                    let L = extra_symbols
                        .make_plt_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    L + A - P
                }
                RelocationKind::ImageOffset => S + A - Image,
                RelocationKind::SectionOffset => S + A - Section,
                _ => panic!("rela: {:?}", rela),
            };

            if rela.size() > 0 {
                // SAFETY: P must be pointing to a valid address
                unsafe {
                    ptr::copy_nonoverlapping(
                        &value as *const i64 as *const u8,
                        P as *mut u8,
                        (rela.size() / 8) as usize,
                    );
                }
            }
        }
    }
}
