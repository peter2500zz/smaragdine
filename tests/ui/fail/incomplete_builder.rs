//@edition: 2024
#![allow(unused_imports, dead_code)]
use smaragdine::{};
#[path = "../support.rs"]
mod support;
use smaragdine::prelude::*;
struct Builder;
impl CommandBuilder for Builder {
    type Source = ();
    type Output = i32;
    type Kind = support::Parser;
}
fn main() {}
