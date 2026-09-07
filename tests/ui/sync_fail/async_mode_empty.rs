//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, async {});
}
