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
    AdmissionLimits, Aggregate, ConflictAction, ConflictTarget, FieldRef, JoinKind, Mutation,
    NamedAggregate, NamedScalar, OrderDirection, OrderKey, Rel, Scalar, ScalarOp, SetOp,
    admit_mutation, admit_query,
};
use sim_relation_schema::{
    AcceptAllValues, ColumnBuilder, Constraint, ForeignKey, PhysicalColumn, PhysicalSchema,
    PhysicalTable, PrimaryKey, Schema, SchemaBuilder, TableBuilder,
};
use sim_relation_site::{Bindings, Limits, SiteError, VecRowSink};
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrialBalanceData {
    pub account: i64,
    pub sru_plus: Option<i32>,
    pub sru_minus: Option<i32>,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub closing_minor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReportBalanceData {
    pub year: i32,
    pub key: i64,
    pub amount: i64,
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
    pub(crate) fn open_report(
        factory: &dyn YearFileFactory,
        mount: Arc<dyn HostDirPort>,
        years: &[i32],
    ) -> Result<Self, StoreError> {
        let sources = years
            .iter()
            .enumerate()
            .map(|(index, year)| {
                let name = if index == 0 {
                    "main".into()
                } else {
                    format!("year_{year}")
                };
                (name, year_leaf(*year))
            })
            .collect::<Vec<_>>();
        Self::from_file(factory.open_report(mount, &sources)?, years[0])
    }

    pub fn trial_balance_data(&self) -> Result<Vec<TrialBalanceData>, StoreError> {
        let joined = Rel::Join {
            left: Box::new(scan("main", "account", "account")),
            right: Box::new(scan("main", "posting", "posting")),
            kind: JoinKind::Left,
            on: call(
                ScalarOp::Eq,
                vec![field("account", "number"), field("posting", "account")],
            ),
        };
        let positive = call(
            ScalarOp::Gt,
            vec![field("posting", "minor"), integer_scalar(0)],
        );
        let negative = call(
            ScalarOp::Lt,
            vec![field("posting", "minor"), integer_scalar(0)],
        );
        let conditional = |predicate, value| Scalar::Case {
            branches: vec![(predicate, value)],
            otherwise: Some(Box::new(integer_scalar(0))),
        };
        let grouped = Rel::Group {
            input: Box::new(joined),
            bind: binding("balance"),
            keys: vec![
                named("account", field("account", "number")),
                named("sru_plus", field("account", "sru_plus")),
                named("sru_minus", field("account", "sru_minus")),
            ],
            aggregates: vec![
                aggregate("debit", conditional(positive, field("posting", "minor"))),
                aggregate(
                    "credit",
                    conditional(
                        negative,
                        call(
                            ScalarOp::Sub,
                            vec![integer_scalar(0), field("posting", "minor")],
                        ),
                    ),
                ),
                aggregate(
                    "closing",
                    Scalar::Case {
                        branches: vec![(
                            call(ScalarOp::IsNull, vec![field("posting", "minor")]),
                            integer_scalar(0),
                        )],
                        otherwise: Some(Box::new(field("posting", "minor"))),
                    },
                ),
            ],
            having: Some(call(
                ScalarOp::Ge,
                vec![field("balance", "debit"), integer_scalar(0)],
            )),
        };
        let rows = self.query_rows(Rel::Order {
            input: Box::new(grouped),
            keys: vec![OrderKey {
                scalar: field("balance", "account"),
                direction: OrderDirection::Asc,
            }],
        })?;
        rows.iter()
            .map(|row| {
                Ok(TrialBalanceData {
                    account: cell_i64(row, 0)?,
                    sru_plus: cell_optional_i32(row, 1)?,
                    sru_minus: cell_optional_i32(row, 2)?,
                    debit_minor: cell_i64(row, 3)?,
                    credit_minor: cell_i64(row, 4)?,
                    closing_minor: cell_i64(row, 5)?,
                })
            })
            .collect()
    }

    pub(crate) fn report_balances(
        &self,
        years: &[i32],
        by_sru: bool,
    ) -> Result<Vec<ReportBalanceData>, StoreError> {
        let projections = years
            .iter()
            .enumerate()
            .map(|(index, year)| {
                let source_name = if index == 0 {
                    "main".into()
                } else {
                    format!("year_{year}")
                };
                let joined = Rel::Join {
                    left: Box::new(scan(&source_name, "posting", "posting")),
                    right: Box::new(scan(&source_name, "account", "account")),
                    kind: JoinKind::Inner,
                    on: call(
                        ScalarOp::Eq,
                        vec![field("posting", "account"), field("account", "number")],
                    ),
                };
                let key = if by_sru {
                    Scalar::Case {
                        branches: vec![(
                            call(
                                ScalarOp::Ge,
                                vec![field("posting", "minor"), integer_scalar(0)],
                            ),
                            call(
                                ScalarOp::Coalesce,
                                vec![field("account", "sru_plus"), field("account", "sru_minus")],
                            ),
                        )],
                        otherwise: Some(Box::new(call(
                            ScalarOp::Coalesce,
                            vec![field("account", "sru_minus"), field("account", "sru_plus")],
                        ))),
                    }
                } else {
                    field("account", "number")
                };
                let projected = Rel::Project {
                    input: Box::new(joined),
                    bind: binding("source_row"),
                    fields: vec![
                        named("year", integer_scalar(i64::from(*year))),
                        named("report_key", key),
                        named("minor", field("posting", "minor")),
                    ],
                };
                let filtered = if by_sru {
                    Rel::Filter {
                        input: Box::new(projected),
                        predicate: call(
                            ScalarOp::Not,
                            vec![call(
                                ScalarOp::IsNull,
                                vec![field("source_row", "report_key")],
                            )],
                        ),
                    }
                } else {
                    projected
                };
                let grouped = Rel::Group {
                    input: Box::new(filtered),
                    bind: binding("report"),
                    keys: vec![
                        named("year", field("source_row", "year")),
                        named("report_key", field("source_row", "report_key")),
                    ],
                    aggregates: vec![aggregate("minor", field("source_row", "minor"))],
                    having: Some(call(
                        ScalarOp::Ne,
                        vec![field("report", "minor"), integer_scalar(0)],
                    )),
                };
                Rel::Order {
                    input: Box::new(grouped),
                    keys: vec![
                        OrderKey {
                            scalar: field("report", "year"),
                            direction: OrderDirection::Asc,
                        },
                        OrderKey {
                            scalar: field("report", "report_key"),
                            direction: OrderDirection::Asc,
                        },
                    ],
                }
            })
            .collect::<Vec<_>>();
        let composed = if projections.len() == 1 {
            projections.into_iter().next().unwrap()
        } else {
            Rel::Set {
                op: SetOp::UnionAll,
                inputs: projections,
            }
        };
        self.query_rows(composed)?
            .iter()
            .map(|row| {
                Ok(ReportBalanceData {
                    year: cell_i32(row, 0)?,
                    key: cell_i64(row, 1)?,
                    amount: cell_i64(row, 2)?,
                })
            })
            .collect()
    }

    fn query_rows(&self, rel: Rel) -> Result<Vec<Row>, StoreError> {
        let plan = admit_query(
            rel,
            &self.schema,
            &self.domains,
            empty_type()?,
            AdmissionLimits::default(),
        )
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let bindings = Bindings::new(&empty_type()?, [])?;
        let mut sink = VecRowSink::default();
        self.file
            .borrow_mut()
            .session()
            .query(&plan, &bindings, &self.limits, &mut sink)?;
        Ok(sink.into_rows())
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
            tx.mutate(&plan, &bindings, &self.limits, &mut VecRowSink::default())?;
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
        let mut sink = VecRowSink::default();
        self.file
            .borrow_mut()
            .session()
            .query(&plan, &bindings, &self.limits, &mut sink)?;
        Ok(sink.into_rows())
    }
}

mod schema;
use schema::{
    aggregate, binding, call, cell_i32, cell_i64, cell_optional_i32, cell_optional_i64,
    cell_optional_text, cell_text, column, domains, empty_type, field, field_name, integer,
    integer_scalar, named, nullable_i64, nullable_text, row_type, scan, source, table_name, text,
    year_leaf,
};
pub use schema::{ledger_schema, legacy_adoption_manifest};

#[cfg(test)]
mod tests;
