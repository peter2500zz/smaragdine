//@edition: 2024
#![allow(unused_imports, dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
use smaragdine::prelude::*;
#[derive(Default)]
struct Parser;
impl CommandArgument for Parser {
    type Builder<S, R> = ArgumentBuilder<S, R, Self>;
}
fn main() {}
