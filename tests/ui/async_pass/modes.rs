//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
async fn handler(_: std::sync::Arc<CommandContext<()>>) -> CommandResult {
    Ok(2)
}
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async {
        literal("parent") => {
            run sync: support::handler;
            support::Parser::arg("player") => { run: handler; };
        };
    });
    commands!(dispatcher, {
        literal("override") => { run async: handler; };
    });
    commands!(dispatcher, async {});
    dispatcher.register(command!(async; literal("other") => { run: handler; }));
}
