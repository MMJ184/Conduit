use assert_cmd::Command;
use predicates::prelude::*;

fn conduit() -> Command {
    Command::cargo_bin("conduit").unwrap()
}

#[test]
fn test_help_exits_zero() {
    conduit().arg("--help").assert().success();
}

#[test]
fn test_no_args_shows_help() {
    conduit().assert().failure();
}
