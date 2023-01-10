use object::{RelocationKind, RelocationTarget};

use super::section::{Section, ExtraSymbolSection};
use super::symbol::{SymbolLookupTable, SymbolTable};

#[allow(non_snake_case)]
pub(crate) fn do_relocation(
    image_addr: usize,
    sections: &Vec<Section>,
    local_sym_table: &SymbolTable,
    extra_symbol_sec: &mut ExtraSymbolSection,
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
                        eprintln!(
                            "name: {}, rela.kind: {:?}, A: {}, rela.size: {}",
                            sym.name,
                            rela.kind(),
                            A,
                            rela.size()
                        );
                        let addr = global_sym_table
                            .lookup_symbol_addr(&sym.name)
                            .unwrap_or_else(|| panic!("missing symbol {}", sym.name));
                        eprintln!("addr: {:0x}", addr);
                        addr as u64
                    } else {
                        eprintln!(
                            "name: {}, rela.kind: {:?}, A: {}, rela.size: {}",
                            sym.name,
                            rela.kind(),
                            A,
                            rela.size()
                        );
                        // for local symbols, just read its symbol address
                        // let SymbolSection::Section(section_index) = sym.section else {
                        //     panic!("no such section: {:?}", sym.section);
                        // };
                        // let section = &sections[section_index.0];
                        // section.address + sym.address
                        sym.address
                    }
                }
                RelocationTarget::Section(_sec_index) => todo!("Got a section to relocate"),
                RelocationTarget::Absolute => 0,
                _ => panic!("rela: {:?}", rela),
            };

            let (P, A, S) = (P as i64, A as i64, S as i64);
            let Image = image_addr as i64;
            let Section = sec.address as i64;
            let GotBase = extra_symbol_sec.get_base_address() as i64;
            let value = match rela.kind() {
                RelocationKind::Absolute => S + A,
                RelocationKind::Relative => S + A - P,
                RelocationKind::Got => {
                    let G = extra_symbol_sec
                        .make_got_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    G + A - GotBase
                }
                RelocationKind::GotRelative => {
                    // Pay attention to this kind
                    let G = extra_symbol_sec
                        .make_got_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    // keep this debug code
                    // eprintln!("{:0x} + {} - {:0x} = {:0x}", G, A, P, G + A - P);
                    // unsafe {
                    //     eprintln!(
                    //         "G_content: {:0x?}",
                    //         std::slice::from_raw_parts(G as *const u8, 16)
                    //     );
                    // }
                    G + A - P
                }
                RelocationKind::GotBaseRelative => GotBase + A - P,
                RelocationKind::GotBaseOffset => S + A - GotBase,
                RelocationKind::PltRelative => {
                    let L = extra_symbol_sec
                        .make_plt_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    L + A - P
                }
                RelocationKind::ImageOffset => S + A - Image,
                RelocationKind::SectionOffset => S + A - Section,
                _ => panic!("rela: {:?}", rela),
            };

            unsafe {
                // SAFETY: P must be pointing to a valid and properly aligned address. This is
                // guaranteed if the relocation logic has no issues.
                match rela.size() {
                    64 => (P as *mut u64).write(value as u64),
                    32 => (P as *mut u32).write(value as u32),
                    16 => (P as *mut u16).write(value as u16),
                    8 => (P as *mut u8).write(value as u8),
                    0 => {}
                    _ => panic!("impossible"),
                }
            }
        }
    }
}
