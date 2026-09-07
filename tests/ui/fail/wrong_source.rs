//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let node = support::Parser::arg::<String, i32>("player");
    commands!(dispatcher, { node => {}; });
}
