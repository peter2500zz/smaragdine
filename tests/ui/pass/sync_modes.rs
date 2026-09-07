//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, sync {
        literal("greet") => { run: support::handler; };
        literal("group") => {
            run sync: support::handler;
            integer("count").range(1..=10) => { run: support::handler; };
        };
    });
    commands!(dispatcher, {});
    commands!(dispatcher, sync {});
    dispatcher.register(command!(sync; literal("other") => { run: support::handler; }));
}
