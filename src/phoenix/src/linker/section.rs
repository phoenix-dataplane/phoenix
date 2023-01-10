use std::ptr;

use object::elf;
use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfSection;
use object::read::SymbolIndex;
use object::{ObjectSection, Relocation, SectionFlags, SectionIndex, SectionKind};

use mmap::{Mmap, MmapOptions};

use super::symbol::{ExtraSymbol, Symbol, SymbolTable};
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
    /// For .bss/.tbss sections, we need to allocate extra spaces.
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
            // For .bss/.tbss sections, we need to allocate extra spaces. We'll fill this later.
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
            | SectionKind::Elf(elf::SHT_FINI_ARRAY)
            | SectionKind::Tls => true,
            _ => false,
        }
    }

    /// Update runtime address for sections needed to load. Allocate memory for .bss sections
    /// if encountered.
    pub(crate) fn update_runtime_addr(&mut self, image_start: *const u8) -> Result<(), Error> {
        if self.kind.is_bss() && self.size > 0 {
            // Allocate memory for .bss/.tbss section.
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
            self.mmap = Some(mmap);
        } else if self.need_load() {
            let file_off = self.file_range.expect("impossible").0;
            self.address = unsafe { image_start.offset(file_off as isize) }.addr() as u64;
            eprintln!(
                "section: {}, {:0x}, image_addr: {:0x}, file_off: {:0x}",
                self.name,
                self.address,
                image_start.addr(),
                file_off
            );
            eprintln!("code: {:?}", unsafe {
                std::slice::from_raw_parts(self.address as *const u8, 32)
            });

            // Initialize tls_begin, tp_addr, and dtp_addr
            if self.kind == SectionKind::Tls {
                // On x86, TP (%gs on i386, %fs on x86-64) refers past the end of all TLVs
                // for historical reasons. TLVs are accessed with negative offsets from TP.
            }
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
        assert!((self.used as usize) < mmap.len());
        let ret = unsafe { mmap.as_ptr().offset(self.used) };
        self.used += sym.size as isize;
        ret
    }
}

pub(crate) struct ExtraSymbolSection {
    mmap: Option<Mmap>,
    used: isize,
}

impl ExtraSymbolSection {
    pub(crate) fn new(num_symbols: usize) -> Result<Self, Error> {
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
        self.section_start().addr()
    }

    #[inline]
    pub(crate) fn section_start(&self) -> *mut ExtraSymbol {
        self.mmap
            .as_ref()
            .unwrap()
            .as_ptr()
            .cast::<ExtraSymbol>()
            .cast_mut()
    }

    /// Allocates an GOT entry in the section and returns the address of the entry.
    #[inline]
    pub(crate) fn make_got_entry(&self, sym_addr: usize, sym_index: SymbolIndex) -> usize {
        let start = self.section_start();
        let entry = unsafe { &mut *start.offset(sym_index.0 as isize) };
        *entry = ExtraSymbol {
            addr: sym_addr,
            /* ff 25 f2 ff ff ff    	jmp    *-0xe(%rip)  # where 0xe = 8 + 6 */
            trampoline: [0xFF, 0x25, 0xF2, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        };
        ptr::addr_of!(entry.addr).addr()
    }

    /// Allocates an GOT entry in the section and returns the address of the trampoline code.
    #[inline]
    pub(crate) fn make_plt_entry(&self, sym_addr: usize, sym_index: SymbolIndex) -> usize {
        let start = self.section_start();
        let entry = unsafe { &mut *start.offset(sym_index.0 as isize) };
        *entry = ExtraSymbol {
            addr: sym_addr,
            trampoline: [0xFF, 0x25, 0xF2, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        };
        ptr::addr_of!(entry.trampoline).addr()
    }
}
