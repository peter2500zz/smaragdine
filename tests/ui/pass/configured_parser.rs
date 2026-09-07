//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
use smaragdine::brigadier::parsers::StringArgument;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, {
        StringArgument::GreedyPhrase.into_arg("text") => {
            run: |_| -> CommandResult { Ok(1) };
        };
    });
    assert_eq!(dispatcher.execute("hello world", ()).unwrap(), 1);
}
