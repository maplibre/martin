mod common;

use std::process::Command;

use common::Binaries;

use insta_cmd::assert_cmd_snapshot;

#[test]
#[ignore = "this is an e2e test"]
fn help_martin() {
    let bins = Binaries::new();
    assert_cmd_snapshot!(Command::new(bins.martin).arg("--help"));
}

#[test]
#[ignore = "this is an e2e test"]
fn help_martin_cp() {
    let bins = Binaries::new();
    assert_cmd_snapshot!(Command::new(bins.martin_cp).arg("--help"));
}

#[test]
#[ignore = "this is an e2e test"]
fn help_mbtiles_help() {
    let bins = Binaries::new();
    assert_cmd_snapshot!(Command::new(bins.mbtiles).arg("--help"));
}
