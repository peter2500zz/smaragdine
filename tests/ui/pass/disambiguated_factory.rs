//@edition: 2024
//@run
#![allow(dead_code)]
use smaragdine::{commands};
use smaragdine::brigadier::{
    arguments::{ArgumentType, ParsedValue},
    string_reader::StringReader,
};
use smaragdine::prelude::*;
use std::sync::Arc;

#[derive(Default)]
struct Parser;
impl ArgumentType for Parser {
    fn parse(&self, reader: &mut StringReader) -> Result<Arc<ParsedValue>, CommandSyntaxError> {
        Ok(Arc::new(reader.read_string()?))
    }
}
impl CommandArgument for Parser {
    type Builder<S, R> = ArgumentBuilder<S, R, Self>;
}
impl Parser {
    fn arg(_id: u32) -> Self {
        Self
    }
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, {
        <Parser as CommandArgument>::arg("player") => {
            run: |_| -> CommandResult { Ok(1) };
        };
    });
    assert_eq!(dispatcher.execute("Bob", ()).unwrap(), 1);
}
