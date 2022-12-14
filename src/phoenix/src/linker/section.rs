use std::alloc::Layout;

use object::elf;
use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfSection;
use object::{ObjectSection, Relocation, SectionFlags, SectionIndex, SectionKind};

use mmap::MmapAligned;

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
    pub(crate) mmap: Option<MmapAligned>,
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
            let layout = Layout::from_size_align(self.size as usize, self.align as usize)?
                .align_to(page_size())?;
            todo!("update the permission");
            let mmap = MmapAligned::map_anon(layout)?;
            self.address = mmap.0.as_ptr().addr() as u64;
        } else if self.need_load() {
            let file_off = self.file_range.expect("impossible").0;
            self.address = unsafe { image_addr.offset(file_off as isize) }.addr() as u64;
        }
        Ok(())
    }
}

fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}
