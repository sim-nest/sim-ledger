#![forbid(unsafe_code)]
//! Deterministic model mount used only by ledger conformance tests.
use sim_storage_port::{
    Cancellation, HostDirError, HostDirErrorKind, HostDirPort, HostEntry, HostEntryKind, PortResult,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct ModelMount {
    state: Arc<Mutex<BTreeMap<Vec<String>, Vec<u8>>>>,
    label: String,
}
impl ModelMount {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(BTreeMap::new())),
            label: label.into(),
        }
    }
}
impl HostDirPort for ModelMount {
    fn label(&self) -> &str {
        &self.label
    }
    fn list(&self, dir: &[String]) -> PortResult<Vec<HostEntry>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .iter()
            .filter(|(p, _)| p.len() == dir.len() + 1 && p.starts_with(dir))
            .map(|(p, b)| HostEntry {
                name: p.last().unwrap().clone(),
                kind: HostEntryKind::File,
                len: b.len() as u64,
            })
            .collect())
    }
    fn metadata(&self, path: &[String]) -> PortResult<Option<HostEntry>> {
        Ok(self.state.lock().unwrap().get(path).map(|b| HostEntry {
            name: path.last().cloned().unwrap_or_default(),
            kind: HostEntryKind::File,
            len: b.len() as u64,
        }))
    }
    fn read(&self, path: &[String]) -> PortResult<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| HostDirError::new(HostDirErrorKind::NotFound, "model entry not found"))
    }
    fn replace(&self, path: &[String], bytes: &[u8], cancel: &dyn Cancellation) -> PortResult<()> {
        if cancel.is_cancelled() {
            return Err(HostDirError::new(HostDirErrorKind::Cancelled, "cancelled"));
        }
        self.state
            .lock()
            .unwrap()
            .insert(path.to_vec(), bytes.to_vec());
        Ok(())
    }
    fn remove_file(&self, path: &[String]) -> PortResult<()> {
        self.state
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| HostDirError::new(HostDirErrorKind::NotFound, "model entry not found"))
    }
    fn create_dir(&self, _: &[String]) -> PortResult<()> {
        Ok(())
    }
    fn remove_dir_all(&self, path: &[String]) -> PortResult<()> {
        self.state
            .lock()
            .unwrap()
            .retain(|p, _| !p.starts_with(path));
        Ok(())
    }
    fn child(&self, name: &str) -> PortResult<Arc<dyn HostDirPort>> {
        Ok(Arc::new(Prefixed {
            inner: self.clone(),
            prefix: vec![name.into()],
        }))
    }
}
#[derive(Clone)]
struct Prefixed {
    inner: ModelMount,
    prefix: Vec<String>,
}
impl Prefixed {
    fn path(&self, p: &[String]) -> Vec<String> {
        self.prefix.iter().chain(p).cloned().collect()
    }
}
impl HostDirPort for Prefixed {
    fn label(&self) -> &str {
        self.inner.label()
    }
    fn list(&self, p: &[String]) -> PortResult<Vec<HostEntry>> {
        self.inner.list(&self.path(p))
    }
    fn metadata(&self, p: &[String]) -> PortResult<Option<HostEntry>> {
        self.inner.metadata(&self.path(p))
    }
    fn read(&self, p: &[String]) -> PortResult<Vec<u8>> {
        self.inner.read(&self.path(p))
    }
    fn replace(&self, p: &[String], b: &[u8], c: &dyn Cancellation) -> PortResult<()> {
        self.inner.replace(&self.path(p), b, c)
    }
    fn remove_file(&self, p: &[String]) -> PortResult<()> {
        self.inner.remove_file(&self.path(p))
    }
    fn create_dir(&self, _: &[String]) -> PortResult<()> {
        Ok(())
    }
    fn remove_dir_all(&self, p: &[String]) -> PortResult<()> {
        self.inner.remove_dir_all(&self.path(p))
    }
    fn child(&self, n: &str) -> PortResult<Arc<dyn HostDirPort>> {
        let mut p = self.prefix.clone();
        p.push(n.into());
        Ok(Arc::new(Self {
            inner: self.inner.clone(),
            prefix: p,
        }))
    }
}
