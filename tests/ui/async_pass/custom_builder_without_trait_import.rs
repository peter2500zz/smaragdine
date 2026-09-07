//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::{CommandArgument, CommandContext, CommandDispatcher, CommandResult};
async fn handler(_: std::sync::Arc<CommandContext<()>>) -> CommandResult {
    Ok(2)
}
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async {
        support::Parser::arg("player") => {
            run: handler;
            support::Parser::arg("reason") => { run: handler; };
        };
    });
}
