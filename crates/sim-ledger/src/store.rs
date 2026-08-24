//! Provider-neutral relational persistence for one ledger year file.
#![allow(missing_docs)]

use crate::model::{
    Account, Amount, Posting, Voucher, VoucherBalanceViolation, voucher_balance_violations,
};
use sim_kernel::{Datum, NumberLiteral, Symbol};
pub use sim_ledger_store_port::{RelationYearFile, StoreError, YearFileFactory};
use sim_relation_core::{
    BaseDomain, BindingName, Cell, ColumnName, DomainCatalog, FieldName, FieldType, ProviderName,
    RevisionName, Row, RowType, SchemaName, SourceName, StorageRepr, TableName,
};
use sim_relation_migrate::AdoptionManifest;
use sim_relation_plan::{
    AdmissionLimits, ConflictAction, ConflictTarget, FieldRef, Mutation, NamedScalar,
    OrderDirection, OrderKey, Rel, Scalar, ScalarOp, admit_mutation, admit_query,
};
use sim_relation_schema::{
    AcceptAllValues, ColumnBuilder, Constraint, ForeignKey, PhysicalColumn, PhysicalSchema,
    PhysicalTable, PrimaryKey, Schema, SchemaBuilder, TableBuilder,
};
use sim_relation_site::{Bindings, Limits, RowSink, SiteError};
use sim_storage_port::HostDirPort;
use std::{cell::RefCell, fmt, sync::Arc};

const EMPTY_YEAR: &[u8] = include_bytes!("../fixtures/empty-ledger-year-v1.sqlite");

pub struct YearStore {
    file: RefCell<Box<dyn RelationYearFile>>,
    schema: Schema,
    domains: DomainCatalog,
    limits: Limits,
    pub year: i32,
}
impl YearStore {
    pub fn create(
        factory: &dyn YearFileFactory,
        mount: Arc<dyn HostDirPort>,
        year: i32,
    ) -> Result<Self, StoreError> {
        let store = Self::from_file(factory.create(mount, &year_leaf(year), EMPTY_YEAR)?, year)?;
        store.set_meta("year", &year.to_string())?;
        Ok(store)
    }
    pub fn open(
        factory: &dyn YearFileFactory,
        mount: Arc<dyn HostDirPort>,
        year: i32,
    ) -> Result<Self, StoreError> {
        let store = Self::from_file(factory.open(mount, &year_leaf(year))?, year)?;
        let actual = store
            .meta_value("year")?
            .ok_or_else(|| StoreError::Invalid("year metadata is absent".into()))?;
        if actual.parse::<i32>().ok() != Some(year) {
            return Err(StoreError::Invalid("year identity mismatch".into()));
        }
        Ok(store)
    }
    fn from_file(file: Box<dyn RelationYearFile>, year: i32) -> Result<Self, StoreError> {
        let domains = domains()?;
        Ok(Self {
            file: RefCell::new(file),
            schema: ledger_schema(&domains)?,
            domains,
            limits: Limits::new(100_000, 1_000_000, 256 * 1024 * 1024, 1_000_000)?,
            year,
        })
    }
    pub fn accounts(&self) -> Result<Vec<Account>, StoreError> {
        self.select(
            "account",
            &["number", "name", "note", "sru_plus", "sru_minus"],
            &[],
            &[("number", OrderDirection::Asc)],
            None,
        )?
        .iter()
        .map(|r| {
            Ok(Account {
                number: cell_i64(r, 0)?,
                name: cell_text(r, 1)?.into(),
                note: cell_optional_text(r, 2)?.map(str::to_owned),
                sru_plus: cell_optional_i32(r, 3)?,
                sru_minus: cell_optional_i32(r, 4)?,
            })
        })
        .collect()
    }
    pub fn vouchers(&self) -> Result<Vec<Voucher>, StoreError> {
        self.select(
            "voucher",
            &["id", "source_id", "date", "text"],
            &[],
            &[("id", OrderDirection::Asc)],
            None,
        )?
        .iter()
        .map(|r| {
            Ok(Voucher {
                id: cell_i64(r, 0)?,
                source_id: cell_optional_i64(r, 1)?,
                date: cell_text(r, 2)?.into(),
                text: cell_optional_text(r, 3)?.map(str::to_owned),
            })
        })
        .collect()
    }
    pub fn postings(&self) -> Result<Vec<Posting>, StoreError> {
        self.select(
            "posting",
            &["id", "source_id", "voucher_id", "account", "minor", "text"],
            &[],
            &[("id", OrderDirection::Asc)],
            None,
        )?
        .iter()
        .map(|r| {
            Ok(Posting {
                id: cell_i64(r, 0)?,
                source_id: cell_optional_i64(r, 1)?,
                voucher_id: cell_i64(r, 2)?,
                account: cell_i64(r, 3)?,
                amount: Amount(cell_i64(r, 4)?),
                text: cell_optional_text(r, 5)?.map(str::to_owned),
            })
        })
        .collect()
    }
    pub fn insert_account(&self, v: &Account) -> Result<(), StoreError> {
        self.ensure_mutable()?;
        self.insert(
            "account",
            &["number", "name", "note", "sru_plus", "sru_minus"],
            vec![
                integer(v.number),
                text(&v.name),
                nullable_text(v.note.as_deref()),
                nullable_i64(v.sru_plus.map(i64::from)),
                nullable_i64(v.sru_minus.map(i64::from)),
            ],
        )
    }
    pub fn insert_voucher(&self, v: &Voucher) -> Result<(), StoreError> {
        self.ensure_mutable()?;
        self.insert(
            "voucher",
            &["id", "source_id", "date", "text"],
            vec![
                integer(v.id),
                nullable_i64(v.source_id),
                text(&v.date),
                nullable_text(v.text.as_deref()),
            ],
        )
    }
    pub fn insert_posting(&self, v: &Posting) -> Result<(), StoreError> {
        self.ensure_mutable()?;
        self.insert(
            "posting",
            &["id", "source_id", "voucher_id", "account", "minor", "text"],
            vec![
                integer(v.id),
                nullable_i64(v.source_id),
                integer(v.voucher_id),
                integer(v.account),
                integer(v.amount.0),
                nullable_text(v.text.as_deref()),
            ],
        )
    }
    pub fn set_id_state(&self, k: &str, n: i64) -> Result<(), StoreError> {
        self.ensure_mutable()?;
        self.upsert("id_state", &["kind", "next"], vec![text(k), integer(n)])
    }
    pub fn id_state_value(&self, k: &str) -> Result<Option<i64>, StoreError> {
        self.select("id_state", &["next"], &[("kind", text(k))], &[], Some(1))?
            .first()
            .map(|r| cell_i64(r, 0))
            .transpose()
    }
    pub fn set_meta(&self, k: &str, v: &str) -> Result<(), StoreError> {
        self.upsert("meta", &["key", "value"], vec![text(k), text(v)])
    }
    pub fn meta_value(&self, k: &str) -> Result<Option<String>, StoreError> {
        self.select("meta", &["value"], &[("key", text(k))], &[], Some(1))?
            .first()
            .map(|r| cell_text(r, 0).map(str::to_owned))
            .transpose()
    }
    pub fn is_closed(&self) -> Result<bool, StoreError> {
        Ok(self.meta_value("closing_state")?.as_deref() == Some("closed"))
    }
    pub fn voucher_balance_violations(&self) -> Result<Vec<VoucherBalanceViolation>, StoreError> {
        voucher_balance_violations(&self.vouchers()?, &self.postings()?)
            .map_err(|e| StoreError::Invalid(e.to_string()))
    }
    fn ensure_mutable(&self) -> Result<(), StoreError> {
        if self.is_closed()? {
            Err(StoreError::Closed)
        } else {
            Ok(())
        }
    }
    fn insert(&self, t: &str, cs: &[&str], vs: Vec<Cell>) -> Result<(), StoreError> {
        self.mutate(t, cs, vs, ConflictAction::Fail)
    }
    fn upsert(&self, t: &str, cs: &[&str], vs: Vec<Cell>) -> Result<(), StoreError> {
        let assignments = cs
            .iter()
            .zip(&vs)
            .skip(1)
            .map(|(n, v)| (column(n), Scalar::Literal(v.clone())))
            .collect();
        self.mutate(
            t,
            cs,
            vs,
            ConflictAction::DoUpdate {
                target: ConflictTarget::PrimaryKey,
                assignments,
                predicate: None,
            },
        )
    }
    fn mutate(
        &self,
        t: &str,
        cs: &[&str],
        vs: Vec<Cell>,
        conflict: ConflictAction,
    ) -> Result<(), StoreError> {
        let ty = row_type(cs, &vs)?;
        let row = Row::new(ty.clone(), vs).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let raw = Mutation::Insert {
            table: table_name(t),
            columns: cs.iter().map(|c| column(c)).collect(),
            input: Box::new(Rel::Values {
                bind: binding("input"),
                row_type: ty,
                rows: vec![row],
            }),
            conflict,
            returning: vec![],
        };
        let plan = admit_mutation(
            raw,
            &self.schema,
            &self.domains,
            empty_type()?,
            AdmissionLimits::default(),
        )
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let bindings = Bindings::new(&empty_type()?, [])?;
        let mut file = self.file.borrow_mut();
        file.session().transaction(&mut |tx| {
            tx.mutate(&plan, &bindings, &self.limits, &mut VecSink::default())?;
            Ok(())
        })?;
        file.persist()
    }
    fn select(
        &self,
        t: &str,
        cs: &[&str],
        filters: &[(&str, Cell)],
        order: &[(&str, OrderDirection)],
        limit: Option<u64>,
    ) -> Result<Vec<Row>, StoreError> {
        let bind = "row";
        let mut rel = Rel::Scan {
            source: source("main"),
            table: table_name(t),
            bind: binding(bind),
        };
        for (n, v) in filters {
            rel = Rel::Filter {
                input: Box::new(rel),
                predicate: Scalar::Call(
                    ScalarOp::Eq,
                    vec![field(bind, n), Scalar::Literal(v.clone())],
                ),
            };
        }
        if !order.is_empty() {
            rel = Rel::Order {
                input: Box::new(rel),
                keys: order
                    .iter()
                    .map(|(n, d)| OrderKey {
                        scalar: field(bind, n),
                        direction: *d,
                    })
                    .collect(),
            };
        }
        if limit.is_some() {
            rel = Rel::Limit {
                input: Box::new(rel),
                count: limit,
                offset: 0,
            };
        }
        rel = Rel::Project {
            input: Box::new(rel),
            bind: binding("output"),
            fields: cs
                .iter()
                .map(|n| NamedScalar {
                    name: field_name(n),
                    scalar: field(bind, n),
                })
                .collect(),
        };
        let plan = admit_query(
            rel,
            &self.schema,
            &self.domains,
            empty_type()?,
            AdmissionLimits::default(),
        )
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let bindings = Bindings::new(&empty_type()?, [])?;
        let mut sink = VecSink::default();
        self.file
            .borrow_mut()
            .session()
            .query(&plan, &bindings, &self.limits, &mut sink)?;
        Ok(sink.rows)
    }
}

#[derive(Default)]
struct VecSink {
    rows: Vec<Row>,
}
impl RowSink for VecSink {
    fn push(&mut self, r: Row) -> Result<(), SiteError> {
        self.rows.push(r);
        Ok(())
    }
}
fn year_leaf(y: i32) -> String {
    format!("year-{y}.sqlite")
}
fn text(v: impl Into<String>) -> Cell {
    Cell::new(BaseDomain::Text.id(), Some(Datum::String(v.into())))
}
fn integer(v: i64) -> Cell {
    Cell::new(
        BaseDomain::I64.id(),
        Some(Datum::Number(NumberLiteral {
            domain: Symbol::qualified("core", "i64"),
            canonical: v.to_string(),
        })),
    )
}
fn nullable_text(v: Option<&str>) -> Cell {
    v.map(text)
        .unwrap_or_else(|| Cell::null(BaseDomain::Text.id()))
}
fn nullable_i64(v: Option<i64>) -> Cell {
    v.map(integer)
        .unwrap_or_else(|| Cell::null(BaseDomain::I64.id()))
}
fn cell_text(r: &Row, i: usize) -> Result<&str, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::String(v)) => Ok(v),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
fn cell_optional_text(r: &Row, i: usize) -> Result<Option<&str>, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::String(v)) => Ok(Some(v)),
        None => Ok(None),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
fn cell_i64(r: &Row, i: usize) -> Result<i64, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::Number(v)) => v
            .canonical
            .parse()
            .map_err(|_| StoreError::Storage(SiteError::Conversion)),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
fn cell_optional_i64(r: &Row, i: usize) -> Result<Option<i64>, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::Number(v)) => v
            .canonical
            .parse()
            .map(Some)
            .map_err(|_| StoreError::Storage(SiteError::Conversion)),
        None => Ok(None),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
fn cell_optional_i32(r: &Row, i: usize) -> Result<Option<i32>, StoreError> {
    cell_optional_i64(r, i)?
        .map(|v| i32::try_from(v).map_err(|_| StoreError::Storage(SiteError::Conversion)))
        .transpose()
}
fn name<T: TryFrom<Symbol>>(v: &str) -> T
where
    T::Error: fmt::Debug,
{
    T::try_from(Symbol::new(v)).expect("static relation name")
}
fn table_name(v: &str) -> TableName {
    name(v)
}
fn column(v: &str) -> ColumnName {
    name(v)
}
fn field_name(v: &str) -> FieldName {
    name(v)
}
fn binding(v: &str) -> BindingName {
    name(v)
}
fn source(v: &str) -> SourceName {
    name(v)
}
fn field(b: &str, n: &str) -> Scalar {
    Scalar::Field(FieldRef {
        binding: binding(b),
        field: field_name(n),
    })
}
fn empty_type() -> Result<RowType, StoreError> {
    RowType::new([]).map_err(|e| StoreError::Invalid(e.to_string()))
}
fn row_type(ns: &[&str], cs: &[Cell]) -> Result<RowType, StoreError> {
    RowType::new(ns.iter().zip(cs).map(|(n, c)| FieldType {
        name: field_name(n),
        domain: c.domain().clone(),
        nullable: c.value().is_none(),
    }))
    .map_err(|e| StoreError::Invalid(e.to_string()))
}
fn domains() -> Result<DomainCatalog, StoreError> {
    DomainCatalog::new([BaseDomain::I64.spec(), BaseDomain::Text.spec()])
        .map_err(|e| StoreError::Invalid(e.to_string()))
}

pub fn ledger_schema(domains: &DomainCatalog) -> Result<Schema, StoreError> {
    let req = |n, d| ColumnBuilder::required(column(n), d).build();
    let nul = |n, d| ColumnBuilder::nullable(column(n), d).build();
    let pk = |t: &str, cs: &[&str]| {
        Constraint::Primary(PrimaryKey {
            name: name(&format!("{t}_pk")),
            columns: cs.iter().map(|c| column(c)).collect(),
        })
    };
    let account = TableBuilder::new(table_name("account"))
        .column(req("number", BaseDomain::I64.id()))
        .column(req("name", BaseDomain::Text.id()))
        .column(nul("note", BaseDomain::Text.id()))
        .column(nul("sru_plus", BaseDomain::I64.id()))
        .column(nul("sru_minus", BaseDomain::I64.id()))
        .constraint(pk("account", &["number"]))
        .build();
    let voucher = TableBuilder::new(table_name("voucher"))
        .column(req("id", BaseDomain::I64.id()))
        .column(nul("source_id", BaseDomain::I64.id()))
        .column(req("date", BaseDomain::Text.id()))
        .column(nul("text", BaseDomain::Text.id()))
        .constraint(pk("voucher", &["id"]))
        .build();
    let posting = TableBuilder::new(table_name("posting"))
        .column(req("id", BaseDomain::I64.id()))
        .column(nul("source_id", BaseDomain::I64.id()))
        .column(req("voucher_id", BaseDomain::I64.id()))
        .column(req("account", BaseDomain::I64.id()))
        .column(req("minor", BaseDomain::I64.id()))
        .column(nul("text", BaseDomain::Text.id()))
        .constraint(pk("posting", &["id"]))
        .constraint(Constraint::Foreign(ForeignKey {
            name: name("posting_voucher_fk"),
            columns: vec![column("voucher_id")],
            target_table: table_name("voucher"),
            target_columns: vec![column("id")],
        }))
        .constraint(Constraint::Foreign(ForeignKey {
            name: name("posting_account_fk"),
            columns: vec![column("account")],
            target_table: table_name("account"),
            target_columns: vec![column("number")],
        }))
        .build();
    let ids = TableBuilder::new(table_name("id_state"))
        .column(req("kind", BaseDomain::Text.id()))
        .column(req("next", BaseDomain::I64.id()))
        .constraint(pk("id_state", &["kind"]))
        .build();
    let meta = TableBuilder::new(table_name("meta"))
        .column(req("key", BaseDomain::Text.id()))
        .column(req("value", BaseDomain::Text.id()))
        .constraint(pk("meta", &["key"]))
        .build();
    SchemaBuilder::new(name::<SchemaName>("main"))
        .table(account)
        .table(voucher)
        .table(posting)
        .table(ids)
        .table(meta)
        .build(domains, &AcceptAllValues)
        .map_err(|e| StoreError::Invalid(e.to_string()))
}
pub fn legacy_adoption_manifest() -> Result<AdoptionManifest, StoreError> {
    let d = domains()?;
    Ok(AdoptionManifest {
        logical_schema: ledger_schema(&d)?
            .id()
            .map_err(|e| StoreError::Invalid(e.to_string()))?,
        physical_schema: legacy_physical_schema()?
            .id()
            .map_err(|e| StoreError::Invalid(e.to_string()))?,
    })
}
fn legacy_physical_schema() -> Result<PhysicalSchema, StoreError> {
    let col = |n: &str, s, nullable, ordinal| PhysicalColumn {
        name: column(n),
        domain: if s == StorageRepr::I64 {
            BaseDomain::I64.id()
        } else {
            BaseDomain::Text.id()
        },
        storage: s,
        nullable,
        ordinal,
    };
    let table = |n: &str, columns| PhysicalTable {
        name: table_name(n),
        columns,
        indexes: vec![],
    };
    PhysicalSchema::normalize(
        ProviderName::new(Symbol::qualified("relation/provider", "sqlite")).unwrap(),
        name::<SchemaName>("main"),
        name::<RevisionName>("ledger-year-v1"),
        vec![
            table(
                "account",
                vec![
                    col("number", StorageRepr::I64, true, 0),
                    col("name", StorageRepr::Text, false, 1),
                    col("note", StorageRepr::Text, true, 2),
                    col("sru_plus", StorageRepr::I64, true, 3),
                    col("sru_minus", StorageRepr::I64, true, 4),
                ],
            ),
            table(
                "id_state",
                vec![
                    col("kind", StorageRepr::Text, true, 0),
                    col("next", StorageRepr::I64, false, 1),
                ],
            ),
            table(
                "meta",
                vec![
                    col("key", StorageRepr::Text, true, 0),
                    col("value", StorageRepr::Text, false, 1),
                ],
            ),
            table(
                "posting",
                vec![
                    col("id", StorageRepr::I64, true, 0),
                    col("source_id", StorageRepr::I64, true, 1),
                    col("voucher_id", StorageRepr::I64, false, 2),
                    col("account", StorageRepr::I64, false, 3),
                    col("minor", StorageRepr::I64, false, 4),
                    col("text", StorageRepr::Text, true, 5),
                ],
            ),
            table(
                "voucher",
                vec![
                    col("id", StorageRepr::I64, true, 0),
                    col("source_id", StorageRepr::I64, true, 1),
                    col("date", StorageRepr::Text, false, 2),
                    col("text", StorageRepr::Text, true, 3),
                ],
            ),
        ],
    )
    .map_err(|e| StoreError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_ledger_test_support::{ModelMount, SqliteYearFileFactory};
    use sim_storage_port::NeverCancel;

    const LEGACY: &[u8] = include_bytes!("../fixtures/legacy-ledger-year-v1.sqlite");

    fn mount() -> Arc<dyn HostDirPort> {
        Arc::new(ModelMount::new("ledger-year-oracle"))
    }

    #[test]
    fn open_and_closed_legacy_year_files_are_complete_oracles() {
        let mount = mount();
        mount
            .replace(&["year-2022.sqlite".into()], LEGACY, &NeverCancel)
            .unwrap();
        let before = mount.read(&["year-2022.sqlite".into()]).unwrap();
        let store = YearStore::open(&SqliteYearFileFactory, mount.clone(), 2022).unwrap();

        assert_eq!(store.accounts().unwrap().len(), 2);
        assert_eq!(
            store
                .vouchers()
                .unwrap()
                .iter()
                .map(|v| v.id)
                .collect::<Vec<_>>(),
            vec![100, 101]
        );
        assert_eq!(store.postings().unwrap().len(), 3);
        assert_eq!(store.id_state_value("voucher").unwrap(), Some(102));
        assert_eq!(
            store.meta_value("source").unwrap().as_deref(),
            Some("legacy")
        );
        assert_eq!(
            store.voucher_balance_violations().unwrap(),
            vec![VoucherBalanceViolation {
                voucher_id: 101,
                posting_count: 1,
                minor_sum: 50
            }]
        );

        let rejected = store.insert_voucher(&Voucher {
            id: 102,
            source_id: None,
            date: "2022-06-01".into(),
            text: None,
        });
        assert!(matches!(rejected, Err(StoreError::Closed)));
        assert_eq!(mount.read(&["year-2022.sqlite".into()]).unwrap(), before);
    }

    #[test]
    fn create_crud_reopen_and_exclusive_lifecycle_are_exact() {
        let mount = mount();
        let factory = SqliteYearFileFactory;
        let store = YearStore::create(&factory, mount.clone(), 2026).unwrap();
        store
            .insert_account(&Account {
                number: 1910,
                name: "Cash".into(),
                note: None,
                sru_plus: Some(1000),
                sru_minus: Some(1000),
            })
            .unwrap();
        store
            .insert_voucher(&Voucher {
                id: 7,
                source_id: Some(70),
                date: "2026-01-02".into(),
                text: Some("Receipt".into()),
            })
            .unwrap();
        store
            .insert_posting(&Posting {
                id: 8,
                source_id: Some(80),
                voucher_id: 7,
                account: 1910,
                amount: Amount(125),
                text: None,
            })
            .unwrap();
        store.set_id_state("voucher", 8).unwrap();
        store.set_meta("source", "oracle").unwrap();
        assert!(matches!(
            YearStore::create(&factory, mount.clone(), 2026),
            Err(StoreError::AlreadyExists)
        ));
        drop(store);

        let reopened = YearStore::open(&factory, mount, 2026).unwrap();
        assert_eq!(reopened.vouchers().unwrap()[0].source_id, Some(70));
        assert_eq!(reopened.postings().unwrap()[0].amount, Amount(125));
        assert_eq!(reopened.id_state_value("voucher").unwrap(), Some(8));
        assert_eq!(
            reopened.meta_value("source").unwrap().as_deref(),
            Some("oracle")
        );
    }

    #[test]
    fn logical_legacy_and_normalized_schema_ids_cross_check() {
        let manifest = legacy_adoption_manifest().unwrap();
        assert_ne!(manifest.logical_schema, manifest.physical_schema);
        assert_eq!(manifest, legacy_adoption_manifest().unwrap());
    }
}
