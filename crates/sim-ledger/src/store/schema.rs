use super::*;

pub(super) fn year_leaf(y: i32) -> String {
    format!("year-{y}.sqlite")
}
pub(super) fn text(v: impl Into<String>) -> Cell {
    Cell::new(BaseDomain::Text.id(), Some(Datum::String(v.into())))
}
pub(super) fn integer(v: i64) -> Cell {
    Cell::new(
        BaseDomain::I64.id(),
        Some(Datum::Number(NumberLiteral {
            domain: Symbol::qualified("core", "i64"),
            canonical: v.to_string(),
        })),
    )
}
pub(super) fn nullable_text(v: Option<&str>) -> Cell {
    v.map(text)
        .unwrap_or_else(|| Cell::null(BaseDomain::Text.id()))
}
pub(super) fn nullable_i64(v: Option<i64>) -> Cell {
    v.map(integer)
        .unwrap_or_else(|| Cell::null(BaseDomain::I64.id()))
}
pub(super) fn cell_text(r: &Row, i: usize) -> Result<&str, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::String(v)) => Ok(v),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
pub(super) fn cell_optional_text(r: &Row, i: usize) -> Result<Option<&str>, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::String(v)) => Ok(Some(v)),
        None => Ok(None),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
pub(super) fn cell_i64(r: &Row, i: usize) -> Result<i64, StoreError> {
    match r.cells().get(i).and_then(Cell::value) {
        Some(Datum::Number(v)) => v
            .canonical
            .parse()
            .map_err(|_| StoreError::Storage(SiteError::Conversion)),
        _ => Err(StoreError::Storage(SiteError::Conversion)),
    }
}
pub(super) fn cell_optional_i64(r: &Row, i: usize) -> Result<Option<i64>, StoreError> {
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
pub(super) fn cell_optional_i32(r: &Row, i: usize) -> Result<Option<i32>, StoreError> {
    cell_optional_i64(r, i)?
        .map(|v| i32::try_from(v).map_err(|_| StoreError::Storage(SiteError::Conversion)))
        .transpose()
}
pub(super) fn cell_i32(r: &Row, i: usize) -> Result<i32, StoreError> {
    i32::try_from(cell_i64(r, i)?).map_err(|_| StoreError::Storage(SiteError::Conversion))
}
pub(super) fn name<T: TryFrom<Symbol>>(v: &str) -> T
where
    T::Error: fmt::Debug,
{
    T::try_from(Symbol::new(v)).expect("static relation name")
}
pub(super) fn table_name(v: &str) -> TableName {
    name(v)
}
pub(super) fn column(v: &str) -> ColumnName {
    name(v)
}
pub(super) fn field_name(v: &str) -> FieldName {
    name(v)
}
pub(super) fn binding(v: &str) -> BindingName {
    name(v)
}
pub(super) fn source(v: &str) -> SourceName {
    name(v)
}
pub(super) fn field(b: &str, n: &str) -> Scalar {
    Scalar::Field(FieldRef {
        binding: binding(b),
        field: field_name(n),
    })
}
pub(super) fn scan(source_name: &str, table: &str, bind: &str) -> Rel {
    Rel::Scan {
        source: source(source_name),
        table: table_name(table),
        bind: binding(bind),
    }
}
pub(super) fn call(op: ScalarOp, values: Vec<Scalar>) -> Scalar {
    Scalar::Call(op, values)
}
pub(super) fn integer_scalar(value: i64) -> Scalar {
    Scalar::Literal(integer(value))
}
pub(super) fn named(name: &str, scalar: Scalar) -> NamedScalar {
    NamedScalar {
        name: field_name(name),
        scalar,
    }
}
pub(super) fn aggregate(name: &str, scalar: Scalar) -> NamedAggregate {
    NamedAggregate {
        name: field_name(name),
        aggregate: Aggregate::Sum(scalar),
    }
}
pub(super) fn empty_type() -> Result<RowType, StoreError> {
    RowType::new([]).map_err(|e| StoreError::Invalid(e.to_string()))
}
pub(super) fn row_type(ns: &[&str], cs: &[Cell]) -> Result<RowType, StoreError> {
    RowType::new(ns.iter().zip(cs).map(|(n, c)| FieldType {
        name: field_name(n),
        domain: c.domain().clone(),
        nullable: c.value().is_none(),
    }))
    .map_err(|e| StoreError::Invalid(e.to_string()))
}
pub(super) fn domains() -> Result<DomainCatalog, StoreError> {
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
pub(super) fn legacy_physical_schema() -> Result<PhysicalSchema, StoreError> {
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
