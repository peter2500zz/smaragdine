//@edition: 2024
#![allow(dead_code)]
#[macro_use]
#[path = "../../../examples/support/command_macros.rs"]
mod command_macros;
use smaragdine::brigadier::{
    arguments::{ArgumentType, ParsedValue},
    string_reader::StringReader,
};
use smaragdine::prelude::*;
use std::sync::Arc;

#[derive(Default)]
struct Parser;
impl ArgumentType for Parser {
    fn parse(&self, _: &mut StringReader) -> Result<Arc<ParsedValue>, CommandSyntaxError> {
        Ok(Arc::new(()))
    }
}
impl CommandArgument for Parser {
    type Builder<S, R> = ArgumentBuilder<S, R, Self>;
}
impl Parser {
    fn online_only(self) -> Self {
        self
    }
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, { Parser::arg("player").online_only() => {}; });
}
