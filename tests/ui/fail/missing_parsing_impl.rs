//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{};
use smaragdine::prelude::*;
#[derive(Default)]
struct Parser;
impl CommandArgument for Parser {
    type Builder<S, R> = ArgumentBuilder<S, R, Self>;
}
fn main() {}
