use object::{RelocationKind, RelocationTarget, SymbolKind};

use super::section::{ExtraSymbolSection, Section};
use super::symbol::{SymbolLookupTable, SymbolTable};
use super::tls::{PhoenixModId, TlsIndex};

use crate::log;

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
            let mut sym_mod_id = 0;
            let P = sec.address + off;
            let A = rela.addend();
            // let mut rela_size = rela.size();
            let S = match rela.target() {
                RelocationTarget::Symbol(sym_index) => {
                    cur_sym_index = Some(sym_index);
                    let sym = local_sym_table.symbol_by_index(sym_index).unwrap();
                    if sym.is_global {
                        // for global symbols, get its name first
                        // then query the symbol in the global symbol lookup table
                        log::trace!(
                            "name: '{}', sec_name: {}, P's off in sec: {:0x}, rela.kind: {:?}, rela.size: {}",
                            sym.name,
                            sec.name,
                            off,
                            rela.kind(),
                            rela.size(),
                        );
                        if sym.kind == SymbolKind::Tls {
                            // sym could be undefined
                            let ti = global_sym_table
                                .lookup_tls_symbol(&sym.name)
                                .unwrap_or_else(|| panic!("missing TLS symbol {}", sym.name));
                            sym_mod_id = ti.mod_id.0;
                            ti.offset as u64
                        } else {
                            let addr = global_sym_table
                                .lookup_symbol_addr(&sym.name)
                                .unwrap_or_else(|| panic!("missing symbol {}", sym.name));
                            addr as u64
                        }
                    } else {
                        log::trace!(
                            "name: '{}', rela.kind: {:?}, A: {}, rela.size: {}",
                            sym.name,
                            rela.kind(),
                            A,
                            rela.size()
                        );
                        if sym.kind == SymbolKind::Tls {
                            // local TLS symbols, the logic should be similar to
                            // SymbolLookupTable::lookup_tls_symbol()
                            assert!(!sym.is_undefined, "sym: {:?}", sym);
                            sym_mod_id = sym.mod_id;
                        }
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

            let (rela_kind, rela_size) = match rela.kind() {
                RelocationKind::Absolute => (rela.kind(), rela.size()),
                RelocationKind::Relative => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_PC64) => (RelocationKind::Relative, 64),
                RelocationKind::Got => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_GOT64) => (RelocationKind::Got, 64),
                RelocationKind::GotRelative => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_GOTPCREL64) => {
                    (RelocationKind::GotRelative, 64)
                }
                RelocationKind::GotBaseRelative => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_GOTPC64) => {
                    (RelocationKind::GotBaseRelative, 64)
                }
                RelocationKind::GotBaseOffset => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_GOTOFF64) => {
                    (RelocationKind::GotBaseOffset, 64)
                }
                RelocationKind::PltRelative => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_PLTOFF64) => {
                    (RelocationKind::PltRelative, 64)
                }
                RelocationKind::ImageOffset => (rela.kind(), rela.size()),
                RelocationKind::SectionOffset => (rela.kind(), rela.size()),
                RelocationKind::Elf(object::elf::R_X86_64_TLSGD) => (rela.kind(), 32),
                RelocationKind::Elf(object::elf::R_X86_64_TLSLD) => (rela.kind(), 32),
                RelocationKind::Elf(object::elf::R_X86_64_DTPOFF32) => (rela.kind(), 32),
                RelocationKind::Elf(object::elf::R_X86_64_DTPOFF64) => (rela.kind(), 64),
                RelocationKind::Elf(object::elf::R_X86_64_GOTPCRELX) => (rela.kind(), 32),
                _ => panic!("Unknown relocation kind: {:?}", rela),
            };

            let value = match rela_kind {
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
                RelocationKind::Elf(object::elf::R_X86_64_TLSGD) => {
                    // 19
                    debug_assert_eq!(rela_size, 32);
                    let ti = TlsIndex {
                        mod_id: PhoenixModId(sym_mod_id),
                        offset: S as usize,
                    };
                    let G = extra_symbol_sec
                        .make_got_entry_for_tls_index(ti, cur_sym_index.expect("sth wrong"))
                        as i64;
                    // eprintln!("{:0x} + {} - {:0x} = {:0x}", G, A, P, G + A - P);
                    // unsafe {
                    //     eprintln!(
                    //         "G_content: {:0x?}",
                    //         std::slice::from_raw_parts(G as *const u8, 16)
                    //     );
                    // }
                    G + A - P
                }
                RelocationKind::Elf(object::elf::R_X86_64_TLSLD) => {
                    // 20
                    debug_assert_eq!(rela_size, 32);
                    let ti = TlsIndex {
                        mod_id: PhoenixModId(sym_mod_id),
                        offset: 0,
                    };
                    let G = extra_symbol_sec
                        .make_got_entry_for_tls_index(ti, cur_sym_index.expect("sth wrong"))
                        as i64;
                    G + A - P
                }
                RelocationKind::Elf(object::elf::R_X86_64_DTPOFF32) => {
                    // 21
                    debug_assert_eq!(rela_size, 32);
                    S + A
                }
                RelocationKind::Elf(object::elf::R_X86_64_DTPOFF64) => {
                    // 17
                    debug_assert_eq!(rela_size, 64);
                    S + A
                }
                RelocationKind::Elf(object::elf::R_X86_64_GOTPCRELX) => {
                    // 41
                    let G = extra_symbol_sec
                        .make_got_entry(S as usize, cur_sym_index.expect("sth wrong"))
                        as i64;
                    eprintln!("rela: {:?}, rela_size: {}", rela, rela_size);
                    debug_assert_eq!(rela_size, 32);
                    G + A - P
                }
                _ => panic!("rela: {:?}", rela),
            };

            unsafe {
                // SAFETY: P must be pointing to a valid and properly aligned address. This is
                // guaranteed if the relocation logic has no issues.
                match rela_size {
                    64 => (P as *mut u64).write(value as u64),
                    32 => (P as *mut u32).write(value as u32),
                    16 => (P as *mut u16).write(value as u16),
                    8 => (P as *mut u8).write(value as u8),
                    _ => panic!("impossible, rela_size: {}", rela_size),
                }
            }
        }
    }
}
