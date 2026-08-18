//! 把 brigadier 指令树变成一个带补全、语法高亮与历史的交互式控制台。

mod completer;
mod highlighter;
mod history;
mod inspect;
mod interrupt;
mod keys;
mod menu;
mod printer;
mod prompt;
mod source;
#[cfg(test)]
mod testing;
mod text;
mod theme;
mod util;

pub use printer::Printer;
pub use source::{Context, Source};
pub use text::Text;
pub use theme::{Paint, Piece, Token, default_paint};
