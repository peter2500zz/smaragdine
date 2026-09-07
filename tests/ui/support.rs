#![allow(dead_code)]

use smaragdine::brigadier::{
    arguments::{ArgumentType, ParsedValue},
    string_reader::StringReader,
};
use smaragdine::prelude::*;
use std::sync::Arc;

#[derive(Default)]
pub struct Parser;

impl ArgumentType for Parser {
    fn parse(&self, reader: &mut StringReader) -> Result<Arc<ParsedValue>, CommandSyntaxError> {
        Ok(Arc::new(reader.read_string()?))
    }
}

impl CommandArgument for Parser {
    type Builder<S, R> = Named<S, R>;
}

pub struct Named<S, R>(ArgumentBuilder<S, R, Parser>);

impl<S, R> From<ArgumentBuilder<S, R, Parser>> for Named<S, R> {
    fn from(builder: ArgumentBuilder<S, R, Parser>) -> Self {
        Self(builder)
    }
}

impl<S, R> CommandBuilder for Named<S, R> {
    type Source = S;
    type Output = R;
    type Kind = Parser;

    fn as_builder(&self) -> &ArgumentBuilder<S, R, Parser> {
        &self.0
    }

    fn map_builder(
        self,
        update: impl FnOnce(ArgumentBuilder<S, R, Parser>) -> ArgumentBuilder<S, R, Parser>,
    ) -> Self {
        Self(update(self.0))
    }

    fn into_builder(self) -> ArgumentBuilder<S, R, Parser> {
        self.0
    }
}

impl<S, R> Named<S, R> {
    pub fn online_only(self) -> Self {
        self
    }
}

pub fn handler(_: &CommandContext<()>) -> CommandResult {
    Ok(1)
}
