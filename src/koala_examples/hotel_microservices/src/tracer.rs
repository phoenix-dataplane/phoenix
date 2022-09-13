use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, Error)]
pub enum Error {
    #[error("Entry not found")]
    EntryNotFound,
}

pub struct Tracer {
    proc_latency: HashMap<String, Vec<Duration>>,
    end_to_end_latency: HashMap<String, Vec<Duration>>,
}

impl Tracer {
    const RECORDER_CAPACITY: usize = 10000;

    pub fn new() -> Tracer {
        Tracer {
            proc_latency: HashMap::new(),
            end_to_end_latency: HashMap::new(),
        }
    }

    pub fn new_proc_entry(&mut self, entry: impl AsRef<str>) {
        self.proc_latency.insert(
            entry.as_ref().to_owned(),
            Vec::with_capacity(Self::RECORDER_CAPACITY),
        );
    }

    pub fn new_end_to_end_entry(&mut self, entry: impl AsRef<str>) {
        self.end_to_end_latency.insert(
            entry.as_ref().to_owned(),
            Vec::with_capacity(Self::RECORDER_CAPACITY),
        );
    }

    pub fn record_proc(&mut self, entry: impl AsRef<str>, dura: Duration) -> Result<(), Error> {
        let records = self
            .proc_latency
            .get_mut(entry.as_ref())
            .ok_or(Error::EntryNotFound)?;
        records.push(dura);
        Ok(())
    }

    pub fn record_end_to_end(
        &mut self,
        entry: impl AsRef<str>,
        dura: Duration,
    ) -> Result<(), Error> {
        let records = self
            .end_to_end_latency
            .get_mut(entry.as_ref())
            .ok_or(Error::EntryNotFound)?;
        records.push(dura);
        Ok(())
    }

    pub fn to_csv(&mut self, path: impl AsRef<Path>) -> csv::Result<()> {
        let mut writer = csv::Writer::from_path(path)?;
        for (entry, records) in self.proc_latency.drain() {
            for (idx, record) in records.into_iter().enumerate() {
                let idx_str = idx.to_string();
                let dura_str = record.as_nanos().to_string();
                writer.write_record(&[
                    "Proc",
                    entry.as_str(),
                    idx_str.as_str(),
                    dura_str.as_str(),
                ])?;
            }
        }

        for (entry, records) in self.end_to_end_latency.drain() {
            for (idx, record) in records.into_iter().enumerate() {
                let idx_str = idx.to_string();
                let dura_str = record.as_nanos().to_string();
                writer.write_record(&[
                    "EndToEnd",
                    entry.as_str(),
                    idx_str.as_str(),
                    dura_str.as_str(),
                ])?;
            }
        }
        writer.flush()?;
        Ok(())
    }
}
