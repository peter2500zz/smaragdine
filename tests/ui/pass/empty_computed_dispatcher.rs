//@edition: 2024
//@run
use smaragdine::prelude::*;

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let count = std::cell::Cell::new(0);
    commands!({ count.set(count.get() + 1); &mut dispatcher }, {});
    assert_eq!(count.get(), 1);
    commands!(dispatcher, {});
}
