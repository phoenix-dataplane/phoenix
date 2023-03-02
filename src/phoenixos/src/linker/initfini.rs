//! Initializers and finalizers in an object file.
use object::elf;
use object::SectionKind;

use super::section::Section;

#[derive(Debug, Clone, Copy, PartialEq)]
enum InitFiniKind {
    Init,
    Ctors(u16),
    Ctor,
    InitArrays(u16),
    InitArray,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InitFini {
    // The specification says the function should take no arguments.
    // TODO(cjr): verify that.
    entry: fn(),
    kind: InitFiniKind,
}

impl InitFini {
    pub(crate) fn new(section: &Section) -> Option<Self> {
        // See Note [Initializers and finalizers (ELF)].
        match section.kind {
            SectionKind::Text | SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => {
                // .init -> .ctors.\d+ -> .ctors -> .init_array.\d+ -> .init_array
                match section.name.as_str() {
                    ".init" => todo!(".init"),
                    ".fini" => todo!(".fini"),
                    n if n.starts_with(".ctors") => todo!("{}", n),
                    n if n.starts_with(".dtors") => todo!("{}", n),
                    n if n.starts_with(".init_array") => todo!("{}", n),
                    n if n.starts_with(".fini_array") => todo!("{}", n),
                    _ => return None,
                }
            }
            SectionKind::Elf(elf::SHT_INIT_ARRAY) => todo!("{:?}", section.kind),
            SectionKind::Elf(elf::SHT_FINI_ARRAY) => todo!("{:?}", section.kind),
            _ => return None,
        }
    }
}
