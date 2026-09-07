//@edition: 2024
//@run
use smaragdine::prelude::*;

fn handler(_: &CommandContext<()>) -> CommandResult { Ok(1) }

macro_rules! forward {
    ($dispatcher:ident; $node:expr; $handler:expr) => {
        commands!($dispatcher, {
            $node => { run: $handler; };
            literal("alias").redirect($dispatcher.root.clone()) => {};
        });
    };
}

macro_rules! subtree {
    ($node:expr; $handler:expr) => { command!($node => { run: $handler; }) };
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    forward!(dispatcher; literal("forwarded").describe("test"); handler);
    let node = subtree!(literal("subtree"); handler);
    commands!(dispatcher, { node => {}; });
    assert_eq!(dispatcher.execute("alias forwarded", ()).unwrap(), 1);
    assert_eq!(dispatcher.execute("subtree", ()).unwrap(), 1);
}
