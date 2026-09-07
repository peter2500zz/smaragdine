//@edition: 2024
//@run
use smaragdine::prelude::*;
use std::ops::{Add, Deref, DerefMut};

struct Receiver<'a>(&'a mut CommandDispatcher<()>);

impl Deref for Receiver<'_> {
    type Target = CommandDispatcher<()>;
    fn deref(&self) -> &Self::Target { self.0 }
}

impl DerefMut for Receiver<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.0 }
}

impl Add<()> for Receiver<'_> {
    type Output = Self;
    fn add(self, _: ()) -> Self { self }
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(Receiver(&mut dispatcher) + (), {
        literal("operator") => { run: |_| -> CommandResult { Ok(1) }; };
    });
    assert_eq!(dispatcher.execute("operator", ()).unwrap(), 1);
}
