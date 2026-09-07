//@edition: 2024
use smaragdine::{command};
use smaragdine::prelude::*;

fn main() {
    let _ = command!(async; literal::<(), i32>("parent") => {});
}
