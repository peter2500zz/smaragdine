//@edition: 2024
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, {
        literal("parent") => {
            literal("child") => {};
            run: support::handler;
        };
    });
}
