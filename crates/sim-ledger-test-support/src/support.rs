use sim_kernel::{Datum, Symbol};
use sim_ledger_store_port::{RelationYearFile, StoreError, YearFileFactory};
use sim_platform_sqlite::{PreopenedStores, SqliteDriver};
use sim_relation_core::{BaseDomain, DomainCatalog};
use sim_relation_site::{Driver, Limits, Session};
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

/// SQLite adapter used by ledger tests and embedders that deliberately choose
/// the native SQLite placement. Ledger product code remains provider-neutral.
#[derive(Clone, Default)]
pub struct SqliteYearFileFactory;

impl YearFileFactory for SqliteYearFileFactory {
    fn create(
        &self,
        mount: Arc<dyn HostDirPort>,
        leaf: &str,
        initial: &[u8],
    ) -> Result<Box<dyn RelationYearFile>, StoreError> {
        if mount.metadata(&[leaf.to_owned()])?.is_some() {
            return Err(StoreError::AlreadyExists);
        }
        NativeYearFile::materialize(mount, leaf, initial)
    }
    fn open(
        &self,
        mount: Arc<dyn HostDirPort>,
        leaf: &str,
    ) -> Result<Box<dyn RelationYearFile>, StoreError> {
        let bytes = mount.read(&[leaf.to_owned()])?;
        NativeYearFile::materialize(mount, leaf, &bytes)
    }
    fn open_report(
        &self,
        mount: Arc<dyn HostDirPort>,
        sources: &[(String, String)],
    ) -> Result<Box<dyn RelationYearFile>, StoreError> {
        NativeYearFile::report(mount, sources)
    }
}

struct NativeYearFile {
    mount: Arc<dyn HostDirPort>,
    leaf: String,
    temp: tempfile::NamedTempFile,
    _attached: Vec<tempfile::NamedTempFile>,
    session: Box<dyn Session>,
}
impl NativeYearFile {
    fn materialize(
        mount: Arc<dyn HostDirPort>,
        leaf: &str,
        bytes: &[u8],
    ) -> Result<Box<dyn RelationYearFile>, StoreError> {
        let temp = tempfile::NamedTempFile::new().map_err(host)?;
        std::fs::write(temp.path(), bytes).map_err(host)?;
        let domains = DomainCatalog::new([BaseDomain::I64.spec(), BaseDomain::Text.spec()])
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let reference = Symbol::new("ledger-year");
        let driver = SqliteDriver::new(
            domains,
            PreopenedStores::new([(reference.clone(), temp.path().to_path_buf())]),
        );
        let locator = Datum::Node {
            tag: Symbol::qualified("relation", "preopened"),
            fields: vec![
                (Symbol::new("ref"), Datum::Symbol(reference)),
                (
                    Symbol::new("access"),
                    Datum::Symbol(Symbol::new("read-write")),
                ),
            ],
        };
        let limits = Limits::new(100_000, 1_000_000, 256 * 1024 * 1024, 1_000_000)?;
        let session = driver.connect(&locator, &limits)?;
        Ok(Box::new(Self {
            mount,
            leaf: leaf.into(),
            temp,
            _attached: vec![],
            session,
        }))
    }
    fn report(
        mount: Arc<dyn HostDirPort>,
        sources: &[(String, String)],
    ) -> Result<Box<dyn RelationYearFile>, StoreError> {
        let ((_, main_leaf), attachments) = sources.split_first().ok_or_else(|| {
            StoreError::Invalid("a report requires at least one year source".into())
        })?;
        let main = tempfile::NamedTempFile::new().map_err(host)?;
        std::fs::write(main.path(), mount.read(std::slice::from_ref(main_leaf))?).map_err(host)?;
        let mut attached = Vec::with_capacity(attachments.len());
        for (_, leaf) in attachments {
            let temp = tempfile::NamedTempFile::new().map_err(host)?;
            std::fs::write(temp.path(), mount.read(std::slice::from_ref(leaf))?).map_err(host)?;
            attached.push(temp);
        }
        let main_ref = Symbol::new("ledger-report-main");
        let attachment_refs = attachments
            .iter()
            .zip(&attached)
            .enumerate()
            .map(|(index, (_, temp))| {
                (
                    Symbol::new(format!("ledger-report-{index}")),
                    temp.path().to_path_buf(),
                )
            })
            .collect::<Vec<_>>();
        let domains = DomainCatalog::new([BaseDomain::I64.spec(), BaseDomain::Text.spec()])
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let stores = std::iter::once((main_ref.clone(), main.path().to_path_buf()))
            .chain(attachment_refs.iter().cloned());
        let driver = SqliteDriver::new(domains, PreopenedStores::new(stores));
        let locator = Datum::Node {
            tag: Symbol::qualified("relation", "preopened"),
            fields: vec![
                (Symbol::new("ref"), Datum::Symbol(main_ref)),
                (
                    Symbol::new("access"),
                    Datum::Symbol(Symbol::new("read-only")),
                ),
            ],
        };
        let limits = Limits::new(100_000, 1_000_000, 256 * 1024 * 1024, 1_000_000)?;
        let mut session = driver.connect(&locator, &limits)?;
        for ((name, _), (reference, _)) in attachments.iter().zip(&attachment_refs) {
            session.attach(
                &Datum::Node {
                    tag: Symbol::qualified("relation", "attach"),
                    fields: vec![
                        (
                            Symbol::new("name"),
                            Datum::Symbol(Symbol::new(name.as_str())),
                        ),
                        (Symbol::new("ref"), Datum::Symbol(reference.clone())),
                        (
                            Symbol::new("access"),
                            Datum::Symbol(Symbol::new("read-only")),
                        ),
                    ],
                },
                &limits,
            )?;
        }
        Ok(Box::new(Self {
            mount,
            leaf: main_leaf.clone(),
            temp: main,
            _attached: attached,
            session,
        }))
    }
}
impl RelationYearFile for NativeYearFile {
    fn session(&mut self) -> &mut dyn Session {
        self.session.as_mut()
    }
    fn persist(&mut self) -> Result<(), StoreError> {
        let bytes = std::fs::read(self.temp.path()).map_err(host)?;
        self.mount.replace(
            std::slice::from_ref(&self.leaf),
            &bytes,
            &sim_storage_port::NeverCancel,
        )?;
        Ok(())
    }
}
fn host(e: std::io::Error) -> StoreError {
    StoreError::Invalid(format!("native test adapter: {e}"))
}
