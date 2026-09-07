//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async { literal("hello") => { run: support::handler; }; });
}
