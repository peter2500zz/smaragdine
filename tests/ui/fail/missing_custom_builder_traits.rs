//@edition: 2024
#![allow(dead_code)]
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
struct Builder<S, R>(ArgumentBuilder<S, R, Parser>);
impl CommandArgument for Parser {
    type Builder<S, R> = Builder<S, R>;
}

fn main() {}
