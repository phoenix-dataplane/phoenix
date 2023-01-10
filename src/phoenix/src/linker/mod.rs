//! A runtime-linker for load a libplugin.rlib into the phoenix's address space.
//! It does the following things.
//!     1. Construct a symbol table
//!         - find out the relocation offset
//!     2. Load the plugin rlib
//!         - parse its dependencies and perform the following steps recursively
//!           for all its dependencies
//!         - extract all objects files from the archive
//!         - ld -r to produce a new single relocatable object file
//!         - to be determinted
//!             - load the object if it does provide new public visible symbols
//!             - perform symbol resolving and relocation
//!         - call the constructor and handle potential errors (i.e., init_module)
//!     3. Update the symbol table to include all new symbols
//!
//! To make sure the linker working, it makes several assumptions. These assumptions
//! may not hold under some special circumstances which requires the user to be
//! extreme careful. Specifically, the linker assumes
//! - all objects are compiled with -fPIC.
//! - there is no LTO for the rlibs (double check whether this condition is necessary)
//! - the compiler toolchain of phoenix itself and the plugins are compatible (the same)
//! - the platform is Linux 64-bit little endian. (This assumption can be removed by
//!   improving the code a little).
use std::alloc::LayoutError;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::{Object, ObjectSymbol};
use rustc_demangle::demangle;
use thiserror::Error;

pub(crate) mod symbol;
use symbol::SymbolLookupTable;

pub(crate) mod section;

pub(crate) mod initfini;

pub(crate) mod module;
use module::{LinkedModule, LoadableModule};

pub(crate) mod relocation;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO: {0}")]
    Io(#[from] io::Error),
    #[error("Object error: {0}")]
    Object(#[from] object::Error),
    #[error("Layout error: {0}")]
    Layout(#[from] LayoutError),
    #[error("Fail to identify runtime offset")]
    RuntimeOffset,
    #[error("Error parsing dep file")]
    ParseDepFile,
    #[error("Invalid module path, expect an rlib")]
    NotAnRlib,
    #[error("Invalid dep file path")]
    InvalidDepPath,
    #[error("Fail to extract {0}")]
    ExtractRlib(PathBuf),
    #[error("Fail to do partial linking {0}")]
    PartialLinking(PathBuf),
}

pub(crate) struct Linker {
    /// The binary for phoenix itself.
    binary: Vec<u8>,
    /// The global symbol lookup table.
    pub(crate) global_sym_table: SymbolLookupTable,
    /// The set of loadable module that are current in memory.
    loaded_modules: Vec<LinkedModule>,
    // /// The set of loadable module that are only
    // loaded_roots: Vec<LinkedModule>,
    /// Working directory
    workdir: PathBuf,
    // These crates are dependencies of phoenix itself, so no need to load them again.
    crates_to_skip: HashSet<String>,
}

impl Linker {
    /// Load the binary of the phoenix itself.
    pub(crate) fn new(workdir: PathBuf) -> Result<Self, Error> {
        // Load deps
        let mut dep_path = fs::read_link("/proc/self/exe")?;
        dep_path.set_extension("d");
        let phoenix_deps = Self::load_deps(dep_path)?;
        let crates_to_skip = phoenix_deps.into_iter().collect();

        // Validate the parse the ELF binary of phoenix
        let self_binary = fs::read("/proc/self/exe")?;
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*self_binary)?;
        log::info!("entry: {:0x}", elf.entry());

        let Some(runtime_offset) = get_runtime_offset(&elf) else {
            return Err(Error::RuntimeOffset);
        };
        log::info!("runtime_offset: {:0x}", runtime_offset);

        // Update symbols' addresses to their runtime addresses
        let mut global_sym_table = SymbolLookupTable::new(&elf);
        for sym in global_sym_table.table.values_mut() {
            if sym.is_global && sym.is_definition {
                sym.address = (sym.address as isize + runtime_offset) as u64;
            }
        }

        // for sym in elf.dynamic_symbols() {
        //     let sym_name = format!("{:#?}", sym.name().unwrap());
        //     println!("{}", sym_name);
        // }

        Ok(Linker {
            binary: self_binary,
            global_sym_table,
            loaded_modules: Vec::new(),
            workdir,
            crates_to_skip,
        })
    }

    /// The dependencies for a dep file.
    fn load_deps<P: AsRef<Path>>(dep_path: P) -> Result<Vec<String>, Error> {
        // Parse dependency closure
        let content = fs::read_to_string(dep_path)?;
        let mut all_deps = Vec::new();
        for line in content.lines() {
            // name:[ dep]*
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((_name, deps)) = line.split_once(':') else {
                return Err(Error::ParseDepFile);
            };
            let v: Vec<String> = deps.split(' ').map(|s| s.to_owned()).collect();
            all_deps.extend(v);
        }
        // deduplicate
        all_deps.sort();
        all_deps.dedup();
        // filter out .rs, .d, keep .rlib, .so a,nd transform .rmeta to .rlib
        let mut all_deps: Vec<String> = all_deps
            .into_iter()
            .map(|x| {
                x.strip_suffix(".rmeta")
                    .map_or(x.clone(), |y| y.to_owned() + ".rlib")
            })
            .collect();
        all_deps.retain(|x| x.ends_with(".rlib") || x.ends_with(".so"));
        Ok(all_deps)
    }

    /// Load a given object file into memory.
    pub(crate) fn load_object<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        // TODO(cjr): redirect this to `load_objects(vec![path])`;
        let loaded = LoadableModule::load(path)?;
        loaded.update_global_symbol_table(&mut self.global_sym_table);
        let mut linked = loaded.link(&self.global_sym_table)?;
        linked.run_init();
        self.loaded_modules.push(linked);
        Ok(())
    }

    /// Load a group of object file into memory.
    pub(crate) fn load_objects<P: AsRef<Path>>(&mut self, objects: Vec<P>) -> Result<(), Error> {
        // 1. Load all objects into memory
        let mut loaded_modules = Vec::new();
        for path in &objects {
            let loaded = LoadableModule::load(path)?;
            loaded_modules.push(loaded);
        }

        // 2. Add definitions of all objects to the global symbol table
        for loaded in &loaded_modules {
            loaded.update_global_symbol_table(&mut self.global_sym_table);
        }
        // 3. Perform relocations for every object
        for (loaded, object) in loaded_modules.into_iter().zip(objects) {
            println!("object: {}", object.as_ref().display());
            let mut linked = loaded.link(&self.global_sym_table)?;
            linked.run_init();
            self.loaded_modules.push(linked);
        }
        Ok(())
    }

    /// Load a given `rlib` file into memory.
    pub(crate) fn load_archive<P1: AsRef<Path>, P2: AsRef<Path>>(
        &mut self,
        archive_path: P1,
        dep_path: P2,
    ) -> Result<(), Error> {
        self.load_archive_inner(archive_path.as_ref(), dep_path.as_ref())
    }

    fn load_archive_inner(&mut self, archive_path: &Path, dep_path: &Path) -> Result<(), Error> {
        if archive_path.extension() != Some(OsStr::new("rlib")) {
            return Err(Error::NotAnRlib);
        }
        // Parse dependency closure
        let mut all_deps = Self::load_deps(dep_path)?;
        // also add the target archive
        all_deps.push(archive_path.display().to_string());

        // For each dependency library, extract the archive, `ld -r` to merge all objects
        // into one relocatable object (aka incremental linking or partial linking)
        let objects = self.extract_and_partial_link(&all_deps)?;

        // Load all dependencies into memory, add symbols to global symbol table, and perform
        // relocation for the group of objects
        self.load_objects(objects)?;
        Ok(())
    }

    fn extract_and_partial_link(&mut self, all_deps: &[String]) -> Result<Vec<PathBuf>, Error> {
        use std::process::Command;
        let mut objects = Vec::new();
        for dep in all_deps {
            // skip if already loaded
            if self.crates_to_skip.contains(dep) {
                log::debug!("{} is already loaded, skipping...", dep);
                eprintln!("todo, also check loaded modules");
                continue;
            }

            let lib_path = Path::new(dep);
            eprintln!("lib_path: {}", lib_path.display());
            let dir_name = lib_path.file_stem().ok_or(Error::InvalidDepPath)?;
            let dir_path = self.workdir.join(dir_name);
            let output_obj = dir_path.join(lib_path.with_extension("o").file_name().unwrap());
            // rm -rf <dir_name> && mkdir -p <dir_name>
            if dir_path.try_exists()? {
                fs::remove_dir_all(&dir_path)?;
            }
            fs::create_dir_all(&dir_path)?;
            // ar x <lib_path> --output <dir_name>
            let status = Command::new("ar")
                .current_dir(&dir_path)
                .arg("x")
                .arg(&lib_path)
                .status()
                .expect("ar failed to start");
            if !status.success() {
                return Err(Error::ExtractRlib(lib_path.to_path_buf()));
            }
            // ld -r *.o -o <output_obj>
            let mut obj_collections = Vec::new();
            for entry in fs::read_dir(&dir_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension() == Some(OsStr::new("o"))
                /* OsStr::new(&str) is just a free transmutation */
                {
                    obj_collections.push(path);
                }
            }
            let status = Command::new("ld")
                .current_dir(&dir_path)
                .arg("-r")
                .arg("-o")
                .arg(&output_obj)
                .args(obj_collections)
                .status()
                .expect("ld failed to start");
            if !status.success() {
                return Err(Error::PartialLinking(lib_path.to_path_buf()));
            }
            objects.push(output_obj);
        }
        Ok(objects)
    }
}

fn get_runtime_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (get_runtime_offset as *const ()).expose_addr();
    println!(
        "addr of get_runtime_offset: {:?}",
        get_runtime_offset as *const ()
    );

    let target_sym = format!("{}::get_runtime_offset", module_path!());

    let mut num_matches = 0;
    let mut runtime_offset = None;

    for sym in elf.symbols() {
        let demangled_sym = format!("{:#?}", demangle(sym.name().unwrap()));
        if demangled_sym == target_sym {
            let addr = sym.address();
            runtime_offset = Some(runtime_addr as isize - addr as isize);
            num_matches += 1;
        }
    }

    if num_matches > 1 {
        panic!("FIXME, found {} matches for {}", num_matches, target_sym);
    }

    runtime_offset
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_linker() {
        let workdir = "/tmp/tmp";
        let mut linker = Linker::new(workdir.into()).unwrap();
        println!("{:?}", std::env::current_dir());
        let target_deps_dir = format!("{}/../../target/debug/deps", env!("CARGO_MANIFEST_DIR"));
        linker
            .load_archive(
                format!("{}/libmmap-3047187639e30dd2.rlib", target_deps_dir),
                format!("{}/mmap-3047187639e30dd2.d", target_deps_dir),
            )
            .unwrap();
        let f_eprint = linker
            .global_sym_table
            .lookup_symbol_addr("_ZN3std2io5stdio7_eprint17h5f2ebd38f95a420bE")
            .unwrap();
        println!("f_eprint: {:0x?}", f_eprint);
        println!("f_eprint: {:0x?}", unsafe {
            std::slice::from_raw_parts(f_eprint as *const u8, 128)
        });
        let f_addr = linker
            .global_sym_table
            // .lookup_symbol_addr("_ZN4mmap16test_load_module17h8f26bf5d2a7b7653E")
            .lookup_symbol_addr("test_load_module")
            .unwrap();
        println!("{:0x?}", f_addr);
        println!("{:0x?}", unsafe {
            std::slice::from_raw_parts(f_addr as *const u8, 128)
        });
        // std::thread::sleep(std::time::Duration::from_secs(10000));
        let c = unsafe { (std::mem::transmute::<usize, fn(i32, i32) -> i32>(f_addr))(42, 1) };
        println!("c = {}", c);
        // let _f2_addr = mmap::test_load_module as usize;
    }
    #[test]
    fn test_linker2() {
        let workdir = "/tmp/tmp";
        let mut linker = Linker::new(workdir.into()).unwrap();
        println!("{:?}", std::env::current_dir());
        let target_deps_dir = format!("{}/../../target/debug/deps", env!("CARGO_MANIFEST_DIR"));
        linker
            .load_archive(
                format!("{}/libmmap-3047187639e30dd2.rlib", target_deps_dir),
                format!("{}/mmap-3047187639e30dd2.d", target_deps_dir),
            )
            .unwrap();
    }
}
