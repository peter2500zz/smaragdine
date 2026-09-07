//@edition: 2024
//@run
use smaragdine::prelude::*;

fn handler(_: &CommandContext<()>) -> CommandResult { Ok(1) }

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let events = std::cell::RefCell::new(Vec::new());
    commands!({ events.borrow_mut().push("dispatcher"); &mut dispatcher }, {
        { events.borrow_mut().push("parent"); literal("parent") } => {
            run: { events.borrow_mut().push("parent handler"); handler };
            { events.borrow_mut().push("child"); literal("child") } => {
                run: { events.borrow_mut().push("child handler"); handler };
            };
        };
        { events.borrow_mut().push("sibling"); literal("sibling") } => {
            run: { events.borrow_mut().push("sibling handler"); handler };
        };
    });
    assert_eq!(*events.borrow(), ["dispatcher", "parent", "parent handler", "child", "child handler", "sibling", "sibling handler"]);
    commands!(dispatcher, {
        literal("alias").redirect(dispatcher.root.clone()) => {};
    });
    assert_eq!(dispatcher.execute("alias parent child", ()).unwrap(), 1);
}
