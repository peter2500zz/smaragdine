//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{commands};
use smaragdine::prelude::*;
#[derive(Default)]
struct Parser;
impl smaragdine::brigadier::arguments::ArgumentType for Parser {
    fn parse(
        &self,
        _: &mut smaragdine::brigadier::string_reader::StringReader,
    ) -> Result<std::sync::Arc<smaragdine::brigadier::arguments::ParsedValue>, CommandSyntaxError>
    {
        Ok(std::sync::Arc::new(()))
    }
}
fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, { Parser::arg("player") => {}; });
}
