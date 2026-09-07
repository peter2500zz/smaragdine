//@edition: 2024
use smaragdine::prelude::*;

fn handler(_: &CommandContext<()>) -> CommandResult { Ok(1) }

fn main() {
    let _ = command!(literal("parent") => {
        run: handler;
        literal("child") => {};
        run sync: handler;
    });
}
