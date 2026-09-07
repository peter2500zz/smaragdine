//@edition: 2024
//@run
use smaragdine::prelude::*;

fn handler(_: &CommandContext<()>) -> CommandResult { Ok(1) }

mod run {
    use super::*;
    pub fn factory() -> impl CommandBuilder<Source = (), Output = i32> {
        literal("path")
    }
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let r#run = literal("raw");
    let __smaragdine_node = std::cell::Cell::new(0);
    let tuple = (literal("tuple"),);
    commands!(dispatcher, {
        if true { literal("if") } else { literal("unused") } => { run: handler; };
        match Some("match") { Some(name) => literal(name), None => unreachable!() } => { run: handler; };
        (|name| literal(name))("closure_factory") => { run: handler; };
        run::factory() => { run: handler; };
        r#run.describe("raw identifier builder") => { run: handler; };
        tuple.0 => { run: handler; };
        { __smaragdine_node.set(1); literal("hygiene") } => {
            run: { __smaragdine_node.set(__smaragdine_node.get() + 1); handler };
        };
    });
    assert_eq!(__smaragdine_node.get(), 2);
    for input in ["if", "match", "closure_factory", "path", "raw", "tuple", "hygiene"] {
        assert_eq!(dispatcher.execute(input, ()).unwrap(), 1);
    }
}
