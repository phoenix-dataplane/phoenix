#![feature(strict_provenance)]
use std::fs;

use object::elf::{FileHeader64, SectionHeader64};
use object::endian::LittleEndian;
use object::read::elf::{ElfFile, FileHeader};
use object::{Object, ObjectSection, ObjectSymbol, Relocation};
use rustc_demangle::demangle;

fn get_relocation_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (main as *const ()).expose_addr();
    println!("addr of main: {:?}", main as *const ());

    let sym_main = format!("{}::main", module_path!());

    for section in elf.sections() {
        dbg!(
            section.index(),
            section.address(),
            section.size(),
            section.align(),
            section.file_range(),
            // section.data(),
            // section.compressed_data(),
            // section.uncompressed_data(),
            section.name().unwrap_or(""),
            section.segment_name().unwrap_or(None),
            section.kind(),
            section.flags(),
            section.relocations().collect::<Vec<(u64, Relocation)>>(),
        );
    }

    for sym in elf.symbols() {
        let demangled_sym = format!("{:#?}", demangle(sym.name().unwrap()));
        if demangled_sym == sym_main {
            dbg!(
                sym.index(),
                sym.name().unwrap_or(""),
                sym.address(),
                sym.kind(),
                sym.section(),
                sym.is_undefined(),
                sym.is_definition(),
                sym.is_common(),
                sym.is_weak(),
                sym.scope(),
                sym.is_global(),
                sym.flags(),
            );
            let addr = sym.address();
            return Some(runtime_addr as isize - addr as isize);
        }
    }

    None
}

fn main() -> anyhow::Result<()> {
    let bin_data = fs::read("/proc/self/exe")?;
    // let bin_data = fs::read("/tmp/tmp/libmmap.o")?;
    let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*bin_data)?;
    println!("entry: {:0x}", elf.entry());

    let relocation_offset = get_relocation_offset(&elf);
    println!("relocation_offset: {:0x}", relocation_offset.unwrap());

    Ok(())
}
