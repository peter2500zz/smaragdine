//! 把 brigadier 指令树变成一个带补全、语法高亮与历史的交互式控制台。

mod printer;
mod source;
mod util;

pub use printer::Printer;
pub use source::{Context, Source};
