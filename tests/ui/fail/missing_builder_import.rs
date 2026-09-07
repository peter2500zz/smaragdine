//@edition: 2024
#![allow(unused_imports, dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::{CommandArgument, CommandDispatcher};
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, { support::Parser::arg("player").describe("Target") => {}; });
}
