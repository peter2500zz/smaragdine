//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::{CommandArgument, CommandDispatcher};
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, {
        support::Parser::arg("player") => {
            run: support::handler;
            support::Parser::arg("reason") => { run: support::handler; };
        };
    });
    let node = command!(support::Parser::arg("other") => { run: support::handler; }).online_only();
    dispatcher.register(node);
    assert_eq!(dispatcher.execute("Bob", ()).unwrap(), 1);
}
