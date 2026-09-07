//! Private command-registration syntax shared by the example and its UI tests.
//!
//! `commands!(dispatcher, { node => { run: handler; }; })` defaults to sync.
//! `commands!(dispatcher, async { ... })` requires the async feature. A local
//! `run sync:` or `run async:` overrides only that node, not its children.
//! Each node ends in `;`, uses `=>`, and has at most one `run`, placed before
//! its children. Node expressions are ordinary Rust expressions, so custom
//! builders, method chains, closures, and reusable subtrees remain supported.
//!
//! Handlers return `Result`: use `-> CommandResult` when an otherwise bare
//! `Ok(value)` leaves the error type ambiguous. Sync handlers take
//! `&CommandContext<S>`; async handlers take `Arc<CommandContext<S>>` and
//! return a Send + 'static future. Type and method errors remain rustc errors.
//!
//! Custom parsers implement ArgumentType and CommandArgument; custom fluent
//! wrappers implement CommandBuilder. Import the prelude for methods written
//! in your own expressions. The macros qualify their own trait calls.
//! A configured or non-Default parser uses `parser.into_arg("name")`; a
//! default parser can use `Parser::arg("name")`. Neither is a bare parser node.
//!
//! Diagnostic boundary: malformed Rust expressions, and a `run` placed after
//! child nodes, may be rejected by rustc's expression matcher before a custom
//! fallback can run. These remain compile errors, but do not yet receive a
//! tailored fix. Keep handlers first; only sibling repetition is used on the
//! valid path so wide command trees do not consume extra recursion depth.
//!
//! These macros are still private: this is not a public library API.

#![allow(unused_macros)]

#[cfg(feature = "async")]
macro_rules! command_async {
    ($($expansion:tt)*) => { $($expansion)* };
}

#[cfg(not(feature = "async"))]
#[allow(unused_macros)]
macro_rules! command_async {
    ($($expansion:tt)*) => {
        compile_error!("async command registration requires the async feature; enable smaragdine's `async` feature or use sync registration")
    };
}

// Validate before inspecting handlers: a misspelled mode must fail even for
// an empty tree or when every handler explicitly overrides the default.
macro_rules! command_mode {
    (sync; $($expansion:tt)*) => { $($expansion)* };
    (async; $($expansion:tt)*) => { command_async!($($expansion)*) };
    ($mode:ident; $($expansion:tt)*) => {
        compile_error!(concat!("unknown command mode `", stringify!($mode), "`; expected `sync` or `async`"))
    };
}

macro_rules! command {
    (@handler sync; $node:expr, $handler:expr) => {
        smaragdine::brigadier::builder::CommandBuilder::executes($node, $handler)
    };
    (@handler async; $node:expr, $handler:expr) => {
        command_async!(smaragdine::brigadier::builder::CommandBuilder::executes_async($node, $handler))
    };
    (@children $mode:ident; $node:expr; run: $($rest:tt)*) => {
        compile_error!("a node may declare only one `run`; place it before child nodes")
    };
    (@children $mode:ident; $node:expr; run $override:ident : $($rest:tt)*) => {
        compile_error!("a node may declare only one `run`; place it before child nodes")
    };
    (@children $mode:ident; $node:expr; $($child:expr => { $($body:tt)* };)*) => {{
        let node = $node;
        // Repetition handles siblings; recursion follows tree depth only.
        $( let node = smaragdine::brigadier::builder::CommandBuilder::then(
            node, command!(@body $mode; $child; $($body)*),
        ); )*
        node
    }};
    (@children $mode:ident; $node:expr; $($invalid:tt)*) => {
        compile_error!("invalid child node; expected `node => { ... };`: use `=>` and end every node with `;` (not `,`); put `run: handler;` before children")
    };
    (@body $mode:ident; $node:expr; run: ; $($rest:tt)*) => {
        compile_error!("missing handler after `run:`; expected `run: handler;`")
    };
    (@body $mode:ident; $node:expr; run $override:ident : ; $($rest:tt)*) => {
        compile_error!("missing handler; expected `run sync: handler;` or `run async: handler;`")
    };
    (@body $mode:ident; $node:expr; run: $handler:expr; $($children:tt)*) => {
        command!(@children $mode; command!(@handler $mode; $node, $handler); $($children)*)
    };
    (@body $mode:ident; $node:expr; run $override:ident : $handler:expr; $($children:tt)*) => {
        command_mode!($override;
            command!(@children $mode; command!(@handler $override; $node, $handler); $($children)*)
        )
    };
    (@body $mode:ident; $node:expr; run: $handler:expr) => {
        compile_error!("missing `;` after command handler; expected `run: handler;`")
    };
    (@body $mode:ident; $node:expr; run $override:ident : $handler:expr) => {
        compile_error!("missing `;` after command handler; expected `run sync: handler;` or `run async: handler;`")
    };
    (@body $mode:ident; $node:expr; run: $($invalid:tt)*) => {
        compile_error!("invalid handler declaration; expected `run: handler;`, `run sync: handler;`, or `run async: handler;`")
    };
    (@body $mode:ident; $node:expr; run = $($invalid:tt)*) => {
        compile_error!("invalid handler declaration; expected `run: handler;`, `run sync: handler;`, or `run async: handler;`")
    };
    (@body $mode:ident; $node:expr; run $override:ident $($invalid:tt)*) => {
        compile_error!("invalid handler declaration; expected `run: handler;`, `run sync: handler;`, or `run async: handler;`")
    };
    (@body $mode:ident; $node:expr; $unknown:ident : $($rest:tt)*) => {
        compile_error!(concat!("unknown node entry `", stringify!($unknown), ":`; use `run: handler;` to attach a handler"))
    };
    (@body $mode:ident; $node:expr; $($children:tt)*) => {
        command!(@children $mode; $node; $($children)*)
    };
    (@node $mode:ident; $node:expr => { $($body:tt)* }) => {
        command!(@body $mode; $node; $($body)*)
    };
    (@node $mode:ident; $($invalid:tt)*) => {
        compile_error!("invalid command node; expected `command!(node => { run: handler; })`")
    };
    ($mode:ident; $($body:tt)*) => {
        command_mode!($mode; command!(@node $mode; $($body)*))
    };
    ($($body:tt)*) => {
        command!(@node sync; $($body)*)
    };
}

macro_rules! commands {
    (@register $dispatcher:ident, $mode:ident { run: $($rest:tt)* }) => {
        compile_error!("`run` belongs inside a node body; expected `node => { run: handler; };`")
    };
    (@register $dispatcher:ident, $mode:ident { run $override:ident : $($rest:tt)* }) => {
        compile_error!("`run` belongs inside a node body; expected `node => { run: handler; };`")
    };
    (@register $dispatcher:ident, $mode:ident { $($node:expr => { $($body:tt)* };)* }) => {{
        // Method calls preserve two-phase receiver borrowing, so a node may
        // reference dispatcher.root while it is being registered.
        $( $dispatcher.register(command!(@body $mode; $node; $($body)*)); )*
    }};
    (@register $dispatcher:ident, $mode:ident { $($invalid:tt)* }) => {
        compile_error!("invalid command list; expected `node => { ... };`: use `=>` and end every node with `;` (not `,`)")
    };
    ($dispatcher:ident, { $($body:tt)* }) => {
        commands!(@register $dispatcher, sync { $($body)* })
    };
    ($dispatcher:ident, $mode:ident { $($body:tt)* }) => {
        command_mode!($mode; commands!(@register $dispatcher, $mode { $($body)* }))
    };
    ($dispatcher:expr, { $($body:tt)* }) => {{
        // Evaluate computed receivers once; precompute targets borrowed from it.
        let dispatcher = &mut $dispatcher;
        commands!(@register dispatcher, sync { $($body)* })
    }};
    ($dispatcher:expr, $mode:ident { $($body:tt)* }) => {
        command_mode!($mode; {
            let dispatcher = &mut $dispatcher;
            commands!(@register dispatcher, $mode { $($body)* })
        })
    };
    ($($invalid:tt)*) => {
        compile_error!("expected `commands!(dispatcher, { node => { ... }; })` or `commands!(dispatcher, async { ... })`")
    };
}
