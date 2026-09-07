//@edition: 2024
use smaragdine::prelude::*;

fn main() {
    let _ = command!(literal::<(), i32>("bad"). => {});
}
