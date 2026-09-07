//@edition: 2024
//@run
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let counter = std::cell::Cell::new(0);
    commands!({ counter.set(counter.get() + 1); &mut dispatcher }, {
        { counter.set(counter.get() + 10); literal("once") } => {
            run: { counter.set(counter.get() + 100); support::handler };
        };
    });
    assert_eq!(counter.get(), 111);
    commands!(dispatcher, {
        literal("alias").redirect(dispatcher.root.clone()) => {};
    });
    let built = literal("built").executes(support::handler).build();
    commands!(dispatcher, { built => {}; });
}
