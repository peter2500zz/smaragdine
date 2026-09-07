//@edition: 2024
//@run
use command_ui_renamed_dependencies::{command, commands, prelude::*};

fn handler(_: &CommandContext<()>) -> CommandResult {
    Ok(7)
}

fn main() {
    let mut dispatcher = CommandDispatcher::<()>::new();
    commands!(dispatcher, {
        literal("target") => { run: handler; };
        literal("alias").redirect(dispatcher.root.clone()) => {};
    });
    assert_eq!(dispatcher.execute("alias target", ()).unwrap(), 7);
    dispatcher.register(command!(literal("subtree") => { run: handler; }));
    assert_eq!(dispatcher.execute("subtree", ()).unwrap(), 7);

    #[cfg(feature = "async")]
    {
        async fn handler(_: std::sync::Arc<CommandContext<()>>) -> CommandResult {
            Ok(9)
        }
        commands!(dispatcher, async { literal("async") => { run: handler; }; });
        command_ui_renamed_dependencies::tokio::runtime::Builder::new_current_thread()
            .build().unwrap().block_on(async {
                assert_eq!(dispatcher.execute_async("async", ()).await.unwrap(), 9);
            });
    }
}
