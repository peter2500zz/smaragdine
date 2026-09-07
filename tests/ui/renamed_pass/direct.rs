//@edition: 2024
//@run
use emerald::prelude::*;

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    emerald::commands!(dispatcher, {
        literal("direct") => { run: |_| -> CommandResult { Ok(1) }; };
    });
    assert_eq!(dispatcher.execute("direct", ()).unwrap(), 1);
}
