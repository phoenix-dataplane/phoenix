use std::path::Path;

use anyhow::{bail, Context};

use crate::addon::PhoenixAddon;
use crate::module::PhoenixModule;

use crate::linker::LinkedModule;

// Re-export for plugin dynamic library's use
pub type InitFnResult<T> = anyhow::Result<T>;

// Takes an optional configuration string.
pub type InitModuleFn = fn(Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>>;
pub type InitAddonFn = fn(Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginName {
    /// Regular phoenix plugin (service)
    Module(String),
    /// Phoenix addons
    Addon(String),
}

pub struct Plugin {
    linked: LinkedModule,
    _old: Option<LinkedModule>,
}

impl Plugin {
    pub(crate) fn new(linked: LinkedModule) -> Self {
        Self { linked, _old: None }
    }

    pub(crate) fn init_module(
        &self,
        config_string: Option<&str>,
    ) -> InitFnResult<Box<dyn PhoenixModule>> {
        let func_addr = self
            .linked
            .lookup_symbol_addr("init_module")
            .with_context(|| {
                format!(
                    "Fail to get symbol 'init_module' for {}",
                    self.linked.path().display()
                )
            })?;

        let func = unsafe { std::mem::transmute::<usize, InitModuleFn>(func_addr) };

        func(config_string)
            .with_context(|| format!("Fail to init_module for {}", self.linked.path().display()))
    }

    pub(crate) fn init_addon(
        &self,
        config_string: Option<&str>,
    ) -> InitFnResult<Box<dyn PhoenixAddon>> {
        let func_addr = self
            .linked
            .lookup_symbol_addr("init_addon")
            .with_context(|| {
                format!(
                    "Fail to get symbol 'init_addon' for {}",
                    self.linked.path().display()
                )
            })?;

        let func = unsafe { std::mem::transmute::<usize, InitAddonFn>(func_addr) };

        func(config_string)
            .with_context(|| format!("Fail to init_addon for {}", self.linked.path().display()))
    }

    pub(crate) fn exit_module() {
        eprintln!("TODO exit_module");
    }
    pub(crate) fn exit_addon() {
        eprintln!("TODO exit_addon");
    }

    pub(crate) fn upgrade(self, new: LinkedModule) -> Self {
        let old = self.linked;
        Plugin {
            linked: new,
            _old: Some(old),
        }
    }

    #[inline]
    pub(crate) fn unload_old(&mut self) {
        self._old = None
    }

    #[inline]
    pub(crate) fn rollback(&mut self) {
        if self._old.is_some() {
            self.linked = self._old.take().unwrap();
        }
    }

    /// Load config string from either inline multiline string or a separate path.
    pub(crate) fn load_config<P: AsRef<Path>, S: AsRef<str>>(
        config_path: Option<P>,
        config_string: Option<S>,
    ) -> anyhow::Result<Option<String>> {
        if config_path.is_some() && config_string.is_some() {
            bail!("Please provide either `config_path` or `config_string`, not both");
        }

        if let Some(path) = config_path {
            Ok(Some(std::fs::read_to_string(path)?))
        } else {
            Ok(config_string.map(|x| x.as_ref().to_string()))
        }
    }
}
