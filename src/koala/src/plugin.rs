use std::collections::HashMap;
use std::sync::Arc;

use crate::module::KoalaModule;
use crate::engine::EngineType;

#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Service(String);


pub struct PluginCollection {
    pub modules: HashMap<String, Arc<dyn KoalaModule>>,
    pub engine_registry: HashMap<EngineType, String>,
    pub service_registry: HashMap<String, Vec<EngineType>>,
} 

impl PluginCollection {

}