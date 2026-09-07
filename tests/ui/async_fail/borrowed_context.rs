//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
use smaragdine::prelude::*;
async fn handler(_: &CommandContext<()>) -> CommandResult {
    Ok(1)
}
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async { literal("hello") => { run: handler; }; });
}
