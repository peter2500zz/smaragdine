//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::CommandDispatcher;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, { support::Parser::arg("player") => {}; });
}
