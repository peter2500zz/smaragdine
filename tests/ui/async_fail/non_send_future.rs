//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
use smaragdine::prelude::*;
async fn handler(_: std::sync::Arc<CommandContext<()>>) -> CommandResult {
    let local = std::rc::Rc::new(1);
    smaragdine::tokio::task::yield_now().await;
    Ok(*local)
}
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async { literal("hello") => { run: handler; }; });
}
