use crate::{CommandContext, run};
use sim_ledger_test_support::{ModelMount, SqliteYearFileFactory};
use std::{collections::BTreeMap, sync::Arc};

fn context() -> CommandContext {
    let mount: Arc<dyn sim_storage_port::HostDirPort> = Arc::new(ModelMount::new("cli-model"));
    CommandContext {
        ledger_sets: BTreeMap::from([("books".into(), mount)]),
        imports: BTreeMap::new(),
        year_files: Arc::new(SqliteYearFileFactory),
        wall_clock_ns: 1_767_225_600_000_000_000,
    }
}
#[test]
fn loadable_command_uses_named_model_mount() {
    let context = context();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run(
            &context,
            ["new", "books", "--label", "Household"],
            &mut out,
            &mut err
        ),
        0
    );
    out.clear();
    assert_eq!(run(&context, ["years", "books"], &mut out, &mut err), 0);
    assert!(out.is_empty());
    assert!(err.is_empty());
}
#[test]
fn absent_mount_fails_without_echoing_a_host_path() {
    let context = context();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(run(&context, ["years", "missing"], &mut out, &mut err), 1);
    let text = String::from_utf8(err).unwrap();
    assert!(text.contains("mount missing is not supplied"));
    assert!(!text.contains('/'));
}
