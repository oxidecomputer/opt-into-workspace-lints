// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{path::PathBuf, process::Stdio, collections::BTreeMap};

use clap::Parser;
use serde::Deserialize;
use toml_edit::{DocumentMut, Table};

/// A simple command-line tool for opting all crates in a Cargo workspace into
/// the new workspace-level lints config.
///
/// This does not have a dry-run option, that's what version control is for.
#[derive(Parser)]
struct OptIn {
    /// Path to workspace root.
    root: PathBuf,
}

/// Partial definition of the `cargo metadata` v1 schema. Since serde ignores
/// unknown fields by default, we can get away with this very thin definition.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    manifest_path: PathBuf,
}

fn main() {
    let args = OptIn::parse();

    // Just to reduce surprises, let's canonicalize that thar path before we do
    // anything with it...
    let root = std::fs::canonicalize(args.root).unwrap();

    println!("changing into directory: {}", root.display());
    std::env::set_current_dir(&root).unwrap();

    // Rustup tries to be smart and cause any Rustup proxies we call to use the
    // same toolchain. That isn't what we want! We want to use whatever
    // toolchain is pinned in the target directory.
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    // Collect the metadata JSON output.
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .stderr(Stdio::inherit())
        .output()
        .unwrap();

    // Parse and index it.
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout).unwrap();
    let mut packages_by_id = BTreeMap::new();
    for package in &metadata.packages {
        packages_by_id.insert(&package.id, package);
    }

    // Process all workspace members.
    for member in &metadata.workspace_members {
        let manifest = &packages_by_id[member].manifest_path;
        let manifest_contents = std::fs::read_to_string(manifest).unwrap();
        let mut doc = manifest_contents.parse::<DocumentMut>().unwrap();

        println!("{member}:");

        match doc.entry("lints") {
            toml_edit::Entry::Occupied(_) => (),
            toml_edit::Entry::Vacant(v) => {
                println!("- adding empty lints table");
                v.insert(toml_edit::Item::Table(Table::default()));
            }
        }
        let lints = &mut doc["lints"].as_table_mut().unwrap();
        if let Some(value) = lints.get_mut("workspace") {
            match value.as_bool() {
                Some(false) => {
                    println!("- currently opts out of workspace lints; changing that");
                    *value = true.into();
                }
                Some(true) => {
                    println!("- already opted into workspace lints");
                }
                None => {
                    println!("- HAS BOGUS WORKSPACE KEY");
                    panic!();
                }
            }
        } else {
            println!("- adding new lints.workspace key");
            lints.insert("workspace", true.into());
        }

        let edited_contents = doc.to_string();
        if edited_contents != manifest_contents {
            println!("- writing changes back");
            std::fs::write(manifest, &edited_contents).unwrap();
        }
    }
}
