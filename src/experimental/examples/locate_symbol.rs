#![feature(strict_provenance)]
use std::fs;

use object::elf::{FileHeader64, SectionHeader64};
use object::endian::LittleEndian;
use object::read::elf::{ElfFile, FileHeader};
use object::{Object, ObjectSection, ObjectSymbol, Relocation};
use rustc_demangle::demangle;

fn get_relocation_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (main as *const ()).addr();
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

    // for sym in elf.symbols() {
    for sym in elf.dynamic_symbols() {
        let demangled_sym = format!("{:#?}", demangle(sym.name().unwrap()));
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

    dbg!("before dynamic_relocations!");
    if let Some(dynamic_relocations) = elf.dynamic_relocations() {
        dbg!("dynamic_relocations!");
        for (off, relocation) in dynamic_relocations {
            dbg!(off, relocation);
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    // no normal relocations (because it's binary/dylib, linker already relocates)
    // some dynamic symbols (and are undefined),
    // many dynamic relocations
    let bin_data = fs::read("/proc/self/exe")?;

    // many normal relocations (mostly debug sections),
    // no dynamic symbols,
    // no dynamic relocations
    // let bin_data = fs::read("/tmp/tmp/core/libcore.o")?;

    let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*bin_data)?;
    println!("entry: {:0x}", elf.entry());

    let relocation_offset = get_relocation_offset(&elf);
    // println!("relocation_offset: {:0x}", relocation_offset.unwrap());

    Ok(())
}
