//@edition: 2024
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
use smaragdine::prelude::*;

fn main() {
    let _ = command!(async; literal::<(), i32>("parent") => {});
}
