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
use std::sync::{Arc, Mutex};

use object::elf::FileHeader64;
use object::endian::LittleEndian;
use object::read::elf::ElfFile;
use object::{Object, ObjectSymbol, SymbolKind};
use rustc_demangle::demangle;
use thiserror::Error;

use crate::log;

pub(crate) mod symbol;
use symbol::SymbolLookupTable;

pub(crate) mod section;

pub(crate) mod initfini;

pub(crate) mod module;
pub(crate) use module::LinkedModule;
use module::LoadableModule;

#[cfg(target_arch = "x86_64")]
pub(crate) mod relocation;

pub(crate) mod tls;
use tls::PHOENIX_MOD_INIT_EXEC;

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

/// The set of loadable module that are current in memory.
pub(crate) struct LoadedModules(Mutex<Vec<LinkedModule>>);

pub(crate) static LOADED_MODULES: LoadedModules = LoadedModules::new();

impl Default for LoadedModules {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadedModules {
    const fn new() -> Self {
        LoadedModules(Mutex::new(Vec::new()))
    }

    fn add(&self, linked: LinkedModule) {
        let mut inner = self.0.lock().unwrap();
        inner.push(linked);
    }

    pub(crate) fn find_module_by_name<P: AsRef<Path>>(&self, name: P) -> Option<LinkedModule> {
        let inner = self.0.lock().unwrap();
        let canonicalized_path = name
            .as_ref()
            .canonicalize()
            .expect("failed to canonicalize");
        for linked in inner.iter() {
            if linked.path().as_path() == canonicalized_path {
                // found it
                return Some(Arc::clone(&linked));
            }
        }
        None
    }

    pub(crate) fn clone_tls_initimage(&self, mod_id: usize) -> Option<Box<[u8]>> {
        let inner = self.0.lock().unwrap();
        for linked in inner.iter() {
            if linked.mod_id() == mod_id {
                // found it
                return Some(linked.tls_initimage().as_slice().into());
            }
        }
        None
    }

    pub(crate) fn contains(&self, path: &str) -> bool {
        let inner = self.0.lock().unwrap();
        let path = PathBuf::from(path);
        inner
            .iter()
            .map(|x| x.path())
            .find(|x| x == &&path)
            .is_some()
    }
}

pub(crate) struct Linker {
    // TODO(cjr): remove this field
    /// The binary for phoenix itself.
    _binary: Vec<u8>,
    /// The global symbol lookup table.
    pub(crate) global_sym_table: SymbolLookupTable,
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
        let crates_to_skip = phoenix_deps
            .into_iter()
            // .filter(|x| !x.ends_with("rlib"))
            .filter(|x| x.contains(".rustup/toolchains") || !x.ends_with("rlib"))
            // .filter(|x| {
            //     x.contains("libstd")
            //         || x.contains("libcore")
            //         || x.contains("libcompiler_builtins")
            //         || !x.ends_with("rlib")
            // })
            // .filter(|_| /* filter out everything */ false)
            .collect();

        // Validate the parse the ELF binary of phoenix
        let self_binary = fs::read("/proc/self/exe")?;
        let elf = ElfFile::<FileHeader64<LittleEndian>>::parse(&*self_binary)?;

        let Some(runtime_offset) = get_runtime_offset(&elf) else {
            return Err(Error::RuntimeOffset);
        };
        log::info!("runtime_offset: {:0x}", runtime_offset);

        // Update symbols' addresses to their runtime addresses
        let mut global_sym_table = SymbolLookupTable::new(&elf);
        for sym in global_sym_table.table.values_mut() {
            // normal symbol definitions
            if sym.is_definition {
                sym.address = (sym.address as isize + runtime_offset) as u64;
            }
            // TLS symbol definitions
            if sym.is_global && sym.kind == SymbolKind::Tls && !sym.is_undefined {
                // the mod_id should be 1
                sym.mod_id = PHOENIX_MOD_INIT_EXEC;
                // the sym.address here is the offset into the TLS initialization image,
                // so no need to touch it.
            }
        }

        // for sym in elf.dynamic_symbols() {
        //     let sym_name = format!("{:#?}", sym.name().unwrap());
        //     println!("{}", sym_name);
        // }

        Ok(Linker {
            _binary: self_binary,
            global_sym_table,
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
    #[allow(unused)]
    pub(crate) fn load_object<P1: AsRef<Path>, P2: AsRef<Path>>(
        &mut self,
        path: (P1, P2),
    ) -> Result<(), Error> {
        // TODO(cjr): redirect this to `load_objects(vec![path])`;
        let loaded =
            LoadableModule::load((path.0.as_ref().to_path_buf(), path.1.as_ref().to_path_buf()))?;
        loaded.update_global_symbol_table(&mut self.global_sym_table);
        let mut linked = loaded.link(&self.global_sym_table)?;
        Arc::get_mut(&mut linked)
            .expect("shouldn't have other outstanding references")
            .run_init();
        LOADED_MODULES.add(linked);
        Ok(())
    }

    /// Loads a group of object file into memory.
    pub(crate) fn load_objects<P1: AsRef<Path>, P2: AsRef<Path>>(
        &mut self,
        objects: Vec<(P1, P2)>,
    ) -> Result<(), Error> {
        // 1. Load all objects into memory
        let mut loaded_modules = Vec::new();
        for path in &objects {
            let loaded = LoadableModule::load((
                path.0.as_ref().to_path_buf(),
                path.1.as_ref().to_path_buf(),
            ))?;
            loaded_modules.push(loaded);
        }

        // 2. Add definitions of all objects to the global symbol table
        for loaded in &loaded_modules {
            loaded.update_global_symbol_table(&mut self.global_sym_table);
        }

        // 3. Perform relocations for every object
        for (loaded, object) in loaded_modules.into_iter().zip(objects) {
            log::debug!("loading object: {}", object.1.as_ref().display());
            let mut linked = loaded.link(&self.global_sym_table)?;
            Arc::get_mut(&mut linked)
                .expect("shouldn't have other outstanding references")
                .run_init();
            LOADED_MODULES.add(linked);
        }
        Ok(())
    }

    /// Loads a given `rlib` file into memory and loads its dependencies parsed from the dep file.
    pub(crate) fn load_archive<P1: AsRef<Path>, P2: AsRef<Path>>(
        &mut self,
        archive_path: P1,
        dep_path: P2,
    ) -> Result<LinkedModule, Error> {
        self.load_archive_inner(archive_path.as_ref(), dep_path.as_ref())
    }

    /// Loads a single `rlib`.
    ///
    /// # Safety
    ///
    /// The user must ensure its dependencies has been properly loaded into
    /// memory and initialized.
    #[allow(unused)]
    pub(crate) unsafe fn load_archive_no_dep<P: AsRef<Path>>(
        &mut self,
        archive_path: P,
    ) -> Result<LinkedModule, Error> {
        if archive_path.as_ref().extension() != Some(OsStr::new("rlib")) {
            return Err(Error::NotAnRlib);
        }
        let objects =
            self.extract_and_partial_link(&[archive_path.as_ref().display().to_string()])?;

        self.load_objects(objects)?;
        // SAFETY: the name should be found in loaded modules
        Ok(LOADED_MODULES.find_module_by_name(archive_path).unwrap())
    }

    fn load_archive_inner(
        &mut self,
        archive_path: &Path,
        dep_path: &Path,
    ) -> Result<LinkedModule, Error> {
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

        // SAFETY: the name should be found in loaded modules
        Ok(LOADED_MODULES.find_module_by_name(archive_path).unwrap())
    }

    fn extract_and_partial_link(
        &mut self,
        all_deps: &[String],
    ) -> Result<Vec<(PathBuf, PathBuf)>, Error> {
        use std::process::Command;
        let mut objects = Vec::new();
        for dep in all_deps {
            // skip if already loaded
            if self.crates_to_skip.contains(dep) || LOADED_MODULES.contains(&dep) {
                log::debug!("{} is already loaded, skipping...", dep);
                continue;
            }

            if !dep.ends_with(".rlib") {
                log::warn!("unexpected dep: {}", dep);
                continue;
            }

            let lib_path = Path::new(dep);
            log::debug!("partial linking for lib: {}", lib_path.display());
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
            objects.push((lib_path.canonicalize().unwrap(), output_obj));
        }
        Ok(objects)
    }
}

fn get_runtime_offset(elf: &ElfFile<FileHeader64<LittleEndian>>) -> Option<isize> {
    let runtime_addr = (get_runtime_offset as *const ()).addr();
    log::info!(
        "Addr of get_runtime_offset: {:?}",
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
