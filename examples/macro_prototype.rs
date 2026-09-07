use smaragdine::prelude::*;
#[cfg(any(feature = "async", test))]
use std::sync::Arc;

// These macros intentionally remain private to this executable example.
// They record the syntax experiment without freezing a public library API.
#[cfg(feature = "async")]
macro_rules! async_handler {
    ($node:expr, $handler:expr) => {
        ($node).executes_async($handler)
    };
}

#[cfg(not(feature = "async"))]
#[allow(unused_macros)]
macro_rules! async_handler {
    ($node:expr, $handler:expr) => {
        compile_error!("async command registration requires the async feature")
    };
}

macro_rules! command {
    (@handler sync; $node:expr, $handler:expr) => {
        ($node).executes($handler)
    };
    (@handler async; $node:expr, $handler:expr) => {
        async_handler!($node, $handler)
    };
    (@children $mode:ident; $node:expr; $($child:expr => { $($body:tt)* };)*) => {{
        let node = $node;
        // Repetition handles siblings; recursion follows tree depth only.
        $( let node = node.then(command!($mode; $child => { $($body)* })); )*
        node
    }};
    ($mode:ident; $node:expr => { run: $handler:expr; $($children:tt)* }) => {
        command!(@children $mode;
            command!(@handler $mode; $node, $handler);
            $($children)*)
    };
    ($mode:ident; $node:expr => { run sync: $handler:expr; $($children:tt)* }) => {
        command!(@children $mode;
            command!(@handler sync; $node, $handler);
            $($children)*)
    };
    ($mode:ident; $node:expr => { run async: $handler:expr; $($children:tt)* }) => {
        command!(@children $mode;
            command!(@handler async; $node, $handler);
            $($children)*)
    };
    ($mode:ident; $node:expr => { $($children:tt)* }) => {
        command!(@children $mode; $node; $($children)*)
    };
    ($node:expr => { $($body:tt)* }) => {
        command!(sync; $node => { $($body)* })
    };
}

macro_rules! commands {
    ($dispatcher:ident, { $($body:tt)* }) => {
        commands!($dispatcher, sync { $($body)* })
    };
    ($dispatcher:ident, $mode:ident { $($node:expr => { $($body:tt)* };)* }) => {{
        // Ordinary method calls preserve two-phase receiver borrowing, so a
        // node may reference dispatcher.root while it is being registered.
        $( $dispatcher.register(command!($mode; $node => { $($body)* })); )*
    }};
    ($dispatcher:expr, { $($body:tt)* }) => {
        commands!($dispatcher, sync { $($body)* })
    };
    ($dispatcher:expr, $mode:ident { $($body:tt)* }) => {{
        // A computed dispatcher expression is evaluated once. Its borrow
        // lasts for this block; precompute any targets borrowed from it.
        let dispatcher = &mut $dispatcher;
        commands!(dispatcher, $mode { $($body)* });
    }};
}

fn hello(_: &CommandContext<()>) -> CommandResult {
    Ok(1)
}

fn read_number(ctx: &CommandContext<()>) -> CommandResult {
    Ok(get_integer(ctx, "number").expect("number is present on this path"))
}

fn sync_commands() -> CommandDispatcher<()> {
    let mut dispatcher = CommandDispatcher::new();
    commands!(dispatcher, {
        literal("greet") => { run: hello; };
        literal("list").describe("List entries") => {
            run: hello;
            argument("number", integer()) => { run: read_number; };
        };
        literal("group") => {
            literal("child") => { run: hello; };
        };
    });
    dispatcher
}

#[cfg(feature = "async")]
fn async_commands() -> CommandDispatcher<()> {
    async fn list(_: Arc<CommandContext<()>>) -> CommandResult {
        Ok(2)
    }

    async fn list_number(ctx: Arc<CommandContext<()>>) -> CommandResult {
        smaragdine::tokio::task::yield_now().await;
        read_number(&ctx)
    }

    async fn reversed(ctx: Arc<CommandContext<()>>) -> CommandResult {
        smaragdine::tokio::task::yield_now().await;
        let number = read_number(&ctx)?;
        Ok(
            if get_bool(&ctx, "reversed").expect("reversed is present on this path") {
                -number
            } else {
                number
            },
        )
    }

    let mut dispatcher = CommandDispatcher::new();
    commands!(dispatcher, async {
        literal("greet") => { run sync: hello; };
        literal("list").describe("List entries asynchronously") => {
            run: list;
            argument("number", integer()) => {
                run: list_number;
                argument("reversed", bool()) => { run: reversed; };
            };
        };
        literal("closure") => { run: async |ctx| -> CommandResult {
            smaragdine::tokio::task::yield_now().await;
            assert_eq!(ctx.input(), "closure");
            Ok(3)
        }; };
        literal("group") => {
            run sync: hello;
            // Overriding the parent handler does not change child defaults.
            literal("child") => { run: list; };
        };
    });
    commands!(dispatcher, {
        literal("override") => { run async: list; };
    });
    dispatcher
}

fn main() {
    let dispatcher = sync_commands();
    assert_eq!(dispatcher.execute("greet", ()).unwrap(), 1);
    assert_eq!(dispatcher.execute("list 7", ()).unwrap(), 7);
    assert_eq!(dispatcher.execute("group child", ()).unwrap(), 1);

    #[cfg(feature = "async")]
    {
        let dispatcher = async_commands();
        let runtime = smaragdine::tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            for (input, expected) in [
                ("greet", 1),
                ("list", 2),
                ("list 7", 7),
                ("list 7 true", -7),
                ("closure", 3),
                ("group child", 2),
                ("override", 2),
            ] {
                assert_eq!(dispatcher.execute_async(input, ()).await.unwrap(), expected);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smaragdine::brigadier::{
        arguments::{ArgumentType, ParsedValue},
        string_reader::StringReader,
        suggestion::SuggestionsBuilder,
    };

    #[test]
    fn synchronous_tree_keeps_optional_arguments_and_pure_branches() {
        let dispatcher = sync_commands();
        for (input, expected) in [
            ("greet", 1),
            ("list", 1),
            ("list ", 1),
            ("list 7", 7),
            ("list 7 ", 7),
            ("group child", 1),
        ] {
            assert_eq!(dispatcher.execute(input, ()).unwrap(), expected);
        }
        assert!(dispatcher.execute("group", ()).is_err());
        assert!(dispatcher.execute("list nope", ()).is_err());
    }

    #[test]
    fn reusable_builder_and_handler_expressions_are_evaluated_once() {
        use std::cell::Cell;
        let count = Cell::new(0);
        let mut dispatcher = CommandDispatcher::<()>::new();
        commands!({ count.set(count.get() + 1); &mut dispatcher }, {
            { count.set(count.get() + 10); literal("once") } => {
                run: { count.set(count.get() + 100); hello };
            };
        });
        assert_eq!(count.get(), 111);
        assert_eq!(dispatcher.execute("once", ()).unwrap(), 1);

        let reusable = command!(literal("reusable") => {
            run: |ctx| -> CommandResult {
                assert_eq!(ctx.input(), "reusable");
                Ok(4)
            };
        })
        .describe("A reusable subtree");
        commands!(dispatcher, { reusable => {}; });
        assert_eq!(dispatcher.execute("reusable", ()).unwrap(), 4);
    }

    struct PlayerParser;
    #[derive(Debug, PartialEq)]
    struct Player(String);

    impl ArgumentType for PlayerParser {
        fn parse(&self, reader: &mut StringReader) -> Result<Arc<ParsedValue>, CommandSyntaxError> {
            Ok(Arc::new(Player(reader.read_unquoted_string().to_owned())))
        }
    }

    #[test]
    fn arbitrary_builders_preserve_metadata_suggestions_and_routing() {
        let mut dispatcher = CommandDispatcher::<()>::new();
        commands!(dispatcher, {
            literal("target").describe("Choose a player") => {
                argument("player", PlayerParser)
                    .suggests(|_: CommandContext<()>, builder: SuggestionsBuilder| {
                        builder.suggest("Bob").build()
                    }) => {
                        run: |ctx| -> CommandResult {
                            let player = ctx.argument("player").unwrap()
                                .downcast_ref::<Player>().unwrap();
                            assert_eq!(player, &Player("Bob".to_owned()));
                            Ok(5)
                        };
                    };
            };
            literal("hidden").requires(|_| false) => { run: hello; };
            literal("alias").redirect(dispatcher.root.clone()) => {};
            literal("fork").fork(
                dispatcher.root.clone(),
                Arc::new(|_| Ok(vec![Arc::new(()), Arc::new(())])),
            ) => {};
        });
        assert_eq!(dispatcher.execute("target Bob", ()).unwrap(), 5);
        assert!(dispatcher.execute("hidden", ()).is_err());
        assert_eq!(dispatcher.execute("alias target Bob", ()).unwrap(), 5);
        assert_eq!(dispatcher.execute("fork target Bob", ()).unwrap(), 2);
        let suggestions =
            CommandDispatcher::get_completion_suggestions(dispatcher.parse("target B".into(), ()));
        assert_eq!(suggestions.list()[0].text(), "Bob");
        assert_eq!(
            dispatcher
                .root
                .read()
                .child("target")
                .unwrap()
                .read()
                .description
                .as_deref(),
            Some("Choose a player"),
        );
    }

    #[test]
    fn handlers_keep_custom_errors() {
        let mut dispatcher = CommandDispatcher::<()>::new();
        commands!(dispatcher, {
            literal("ok") => { run: |_| -> CommandResult { Ok(7) }; };
            literal("error") => {
                run: |_| -> Result<i32, std::io::Error> {
                    Err(std::io::Error::other("command failed"))
                };
            };
        });
        assert_eq!(dispatcher.execute("ok", ()).unwrap(), 7);
        assert!(dispatcher.execute("error", ()).is_err());
    }

    #[test]
    fn wide_trees_do_not_recurse_through_siblings() {
        macro_rules! wide {
            ($dispatcher:ident; $($index:literal),* $(,)?) => {
                commands!($dispatcher, {
                    literal("parent") => {
                        $( literal(stringify!($index)) => { run: hello; }; )*
                    };
                    $( literal(concat!("root", stringify!($index))) => { run: hello; }; )*
                });
            };
        }
        let mut dispatcher = CommandDispatcher::<()>::new();
        wide!(dispatcher;
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
            10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
            30, 31, 32, 33, 34, 35, 36, 37, 38, 39,
            40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
            50, 51, 52, 53, 54, 55, 56, 57, 58, 59,
            60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
            80, 81, 82, 83, 84, 85, 86, 87, 88, 89,
            90, 91, 92, 93, 94, 95, 96, 97, 98, 99,
            100, 101, 102, 103, 104, 105, 106, 107, 108, 109,
            110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
            120, 121, 122, 123, 124, 125, 126, 127, 128, 129,
            130, 131, 132, 133, 134, 135, 136, 137, 138, 139,
            140, 141, 142, 143, 144, 145, 146, 147, 148, 149,
            150, 151, 152, 153, 154, 155, 156, 157, 158, 159,
            160, 161, 162, 163, 164, 165, 166, 167, 168, 169,
            170, 171, 172, 173, 174, 175, 176, 177, 178, 179,
            180, 181, 182, 183, 184, 185, 186, 187, 188, 189,
            190, 191, 192, 193, 194, 195, 196, 197, 198, 199,
        );
        for index in 0..200 {
            assert_eq!(
                dispatcher.execute(format!("parent {index}"), ()).unwrap(),
                1
            );
            assert_eq!(dispatcher.execute(format!("root{index}"), ()).unwrap(), 1);
        }
    }

    #[cfg(not(feature = "async"))]
    #[test]
    fn synchronous_build_keeps_rc_contexts_and_non_send_values() {
        use smaragdine::brigadier::context::CommandContextRef;
        use std::rc::Rc;

        struct LocalParser;
        impl ArgumentType for LocalParser {
            #[allow(clippy::arc_with_non_send_sync)]
            fn parse(
                &self,
                reader: &mut StringReader,
            ) -> Result<Arc<ParsedValue>, CommandSyntaxError> {
                reader.skip();
                Ok(Arc::new(Rc::new(99)))
            }
        }
        let mut dispatcher = CommandDispatcher::<()>::new();
        commands!(dispatcher, {
            literal("local") => {
                argument("value", LocalParser) => {
                    run: |ctx| -> CommandResult {
                        Ok(**ctx.argument("value").unwrap()
                            .downcast_ref::<Rc<i32>>().unwrap())
                    };
                };
            };
        });
        let parsed = dispatcher.parse("local x".into(), ());
        let context: Rc<CommandContext<()>> =
            CommandContextRef::new(parsed.context.build("local x"));
        assert_eq!(Rc::strong_count(&context), 1);
        assert_eq!(dispatcher.execute("local x", ()).unwrap(), 99);
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_mode_supports_named_functions_closures_and_local_overrides() {
        let dispatcher = async_commands();
        let runtime = smaragdine::tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            for (input, expected) in [
                ("greet", 1),
                ("list", 2),
                ("list ", 2),
                ("list 7", 7),
                ("list 7 true", -7),
                ("list 7 false", 7),
                ("closure", 3),
                ("group", 1),
                ("group child", 2),
                ("override", 2),
            ] {
                assert_eq!(dispatcher.execute_async(input, ()).await.unwrap(), expected);
            }
        });
        assert_eq!(dispatcher.execute("greet", ()).unwrap(), 1);
        assert!(dispatcher.execute("list", ()).is_err());
    }
}
