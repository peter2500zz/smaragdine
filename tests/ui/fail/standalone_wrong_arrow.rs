//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{command};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    let _ = command!(literal::<(), i32>("hello") -> {});
}
