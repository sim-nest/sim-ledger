//! Workspace package reachability checks.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde::Deserialize;

pub fn run() -> Result<(), String> {
    let metadata = cargo_metadata()?;
    let workspace = metadata
        .workspace_members
        .into_iter()
        .collect::<BTreeSet<_>>();
    let packages = metadata
        .packages
        .into_iter()
        .filter(|package| workspace.contains(&package.id))
        .map(|package| (package.id.clone(), package))
        .collect::<BTreeMap<_, _>>();
    let reverse_dependencies = reverse_dependencies(&packages);
    let orphans = packages
        .values()
        .filter(|package| is_private_package(package))
        .filter(|package| package.name != "xtask")
        .filter(|package| !has_bin_target(package))
        .filter(|package| !has_orphan_exemption(package))
        .filter(|package| {
            reverse_dependencies
                .get(&package.name)
                .is_none_or(BTreeSet::is_empty)
        })
        .map(|package| format!("{} ({})", package.name, package.manifest_path))
        .collect::<Vec<_>>();

    if orphans.is_empty() {
        println!(
            "check-orphan-crates: OK ({} workspace package(s) checked)",
            packages.len()
        );
        Ok(())
    } else {
        Err(format!(
            "unreachable private workspace package(s): {}\n\
             add a local dependent, add a bin target, or record an \
             `orphan-crate-exempt:` reason in the manifest",
            orphans.join(", ")
        ))
    }
}

fn cargo_metadata() -> Result<Metadata, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|err| format!("run cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|err| format!("parse cargo metadata: {err}"))
}

fn reverse_dependencies(
    packages: &BTreeMap<String, Package>,
) -> BTreeMap<String, BTreeSet<String>> {
    let workspace_names = packages
        .values()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for package in packages.values() {
        for dependency in &package.dependencies {
            if dependency.kind.as_deref() != Some("dev")
                && workspace_names.contains(dependency.name.as_str())
            {
                reverse
                    .entry(dependency.name.clone())
                    .or_default()
                    .insert(package.name.clone());
            }
        }
    }
    reverse
}

fn is_private_package(package: &Package) -> bool {
    package.publish.as_ref().is_some_and(Vec::is_empty)
}

fn has_bin_target(package: &Package) -> bool {
    package
        .targets
        .iter()
        .any(|target| target.kind.iter().any(|kind| kind == "bin"))
}

fn has_orphan_exemption(package: &Package) -> bool {
    std::fs::read_to_string(&package.manifest_path)
        .ok()
        .is_some_and(|text| text.contains("orphan-crate-exempt:"))
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
    targets: Vec<Target>,
    manifest_path: String,
    publish: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
}
