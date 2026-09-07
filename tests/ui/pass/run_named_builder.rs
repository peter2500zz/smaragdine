//@edition: 2024
//@run
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
use smaragdine::prelude::*;

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let run = literal("named_run");
    commands!(dispatcher, { run => {}; });
    let run = literal("child");
    commands!(dispatcher, { literal("parent") => { run => {}; }; });
}
