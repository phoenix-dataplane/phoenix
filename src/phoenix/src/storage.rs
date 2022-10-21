use std::collections::HashMap;

use thiserror::Error;
use tokio::sync::mpsc::UnboundedReceiver as CommandReceiver;
use tokio::sync::mpsc::UnboundedSender as CommandSender;

use crate::engine::EngineType;
use crate::envelop::{AnyCommandReceiver, AnyCommandSender};
use crate::envelop::{AnyMessage, AnyResource};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Resource not found")]
    NotFound,
    #[error("Resource already exists")]
    AlreadyExists,
    #[error("Resource downcast fails, type_name: {0}")]
    Downcast(String),
}

pub type ResourceCollection = HashMap<String, Box<dyn AnyResource>>;

pub struct CommandPathBroker {
    senders: HashMap<EngineType, AnyCommandSender>,
    receivers: HashMap<EngineType, AnyCommandReceiver>,
}

impl CommandPathBroker {
    pub(crate) fn new() -> Self {
        CommandPathBroker {
            senders: HashMap::new(),
            receivers: HashMap::new(),
        }
    }
}

impl CommandPathBroker {
    pub fn put_sender<T: AnyMessage>(
        &mut self,
        engine: EngineType,
        sender: CommandSender<T>,
    ) -> Result<(), Error> {
        if self.senders.contains_key(&engine) {
            return Err(Error::AlreadyExists);
        }
        self.senders
            .insert(engine, AnyCommandSender(Box::new(sender)));
        Ok(())
    }

    /// Attemp to downcast the sender
    pub fn get_sender<T: AnyMessage>(
        &mut self,
        engine: &EngineType,
    ) -> Result<CommandSender<T>, Error> {
        let sender = self.senders.remove(engine).ok_or(Error::NotFound)?;
        sender
            .downcast()
            .map_err(|x| Error::Downcast(x.type_name().to_string()))
    }

    /// Downcast the sender without type checking
    ///
    /// # Safety
    /// 
    /// TBD
    pub unsafe fn get_sender_unchecked<T: AnyMessage>(
        &mut self,
        engine: &EngineType,
    ) -> Result<CommandSender<T>, Error> {
        let sender = self.senders.remove(engine).ok_or(Error::NotFound)?;
        let concrete = sender.downcast_unchecked();
        Ok(concrete)
    }

    pub fn get_sender_clone<T: AnyMessage>(
        &self,
        engine: &EngineType,
    ) -> Result<CommandSender<T>, Error> {
        let sender = self.senders.get(engine).ok_or(Error::NotFound)?;
        let cloned = sender
            .downcast_clone()
            .ok_or_else(|| Error::Downcast(sender.type_name().to_string()))?;
        Ok(cloned)
    }

    /// # Safety
    /// 
    /// TBD
    pub unsafe fn get_sender_clone_unchecked<T: AnyMessage>(
        &self,
        engine: &EngineType,
    ) -> Result<CommandSender<T>, Error> {
        let sender = self.senders.get(engine).ok_or(Error::NotFound)?;
        let cloned = sender.downcast_clone_unchecked();
        Ok(cloned)
    }

    pub fn put_receiver<T: AnyMessage>(
        &mut self,
        engine: EngineType,
        receiver: CommandReceiver<T>,
    ) -> Result<(), Error> {
        if self.receivers.contains_key(&engine) {
            return Err(Error::AlreadyExists);
        }
        self.receivers
            .insert(engine, AnyCommandReceiver(Box::new(receiver)));
        Ok(())
    }

    pub fn get_receiver<T: AnyMessage>(
        &mut self,
        engine: &EngineType,
    ) -> Result<CommandReceiver<T>, Error> {
        let receiver = self.receivers.remove(engine).ok_or(Error::NotFound)?;
        receiver
            .downcast()
            .map_err(|x| Error::Downcast(x.type_name().to_string()))
    }

    /// # Safety
    /// 
    /// TBD
    pub unsafe fn get_receiver_unchekced<T: AnyMessage>(
        &mut self,
        engine: &EngineType,
    ) -> Result<CommandReceiver<T>, Error> {
        let receiver = self.receivers.remove(engine).ok_or(Error::NotFound)?;
        let concrete = receiver.downcast_unchecked();
        Ok(concrete)
    }
}

/// Shared storage for sharing resources
/// command path channels, message path channels
/// among a group of engines of a service
/// that serve the same user thread
pub struct SharedStorage {
    pub command_path: CommandPathBroker,
    pub resources: ResourceCollection,
}

impl SharedStorage {
    pub(crate) fn new() -> Self {
        SharedStorage {
            command_path: CommandPathBroker::new(),
            resources: ResourceCollection::new(),
        }
    }
}
