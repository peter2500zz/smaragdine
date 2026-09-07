/// Register a command tree with the optional `macros` feature.
///
/// Each node is an ordinary builder expression followed by `=> { ... };`.
/// A node may have one `run: handler;`, before its children. Empty bodies and
/// branch-only nodes are supported. Custom builders, built nodes, method
/// chains, closures, and reusable subtrees keep their usual Rust semantics.
///
/// ```
/// use smaragdine::prelude::*;
///
/// fn greet(_: &CommandContext<()>) -> CommandResult { Ok(1) }
/// fn count(ctx: &CommandContext<()>) -> CommandResult {
///     Ok(get_integer(ctx, "number").unwrap())
/// }
/// let mut dispatcher = CommandDispatcher::new();
/// commands!(dispatcher, {
///     literal("greet") => { run: greet; };
///     literal("list") => {
///         run: greet;
///         integer("number").range(1..=100) => { run: count; };
///     };
/// });
/// assert_eq!(dispatcher.execute("list ", ()).unwrap(), 1);
/// assert_eq!(dispatcher.execute("list 7", ()).unwrap(), 7);
/// ```
///
/// The default mode is synchronous. `commands!(dispatcher, async { ... })`
/// requires both `macros` and `async`. A local `run sync:` or `run async:`
/// overrides only that node; children inherit the surrounding tree mode.
/// Async handlers take `Arc<CommandContext<S>>` and return a `Send + 'static`
/// future. Named `async fn` handlers and async closures are accepted.
///
/// ```
/// # #[cfg(feature = "async")]
/// # {
/// use smaragdine::prelude::*;
/// use std::sync::Arc;
/// async fn hello(_: Arc<CommandContext<()>>) -> CommandResult { Ok(1) }
/// let mut dispatcher = CommandDispatcher::new();
/// commands!(dispatcher, async {
///     literal("hello") => { run: hello; };
///     literal("local") => { run sync: |_| -> CommandResult { Ok(2) }; };
/// });
/// # }
/// ```
///
/// Node and handler expressions are evaluated once, in depth-first source
/// order: node, handler, then children. Each root is registered before the
/// next root is evaluated. A plain dispatcher identifier preserves two-phase
/// borrowing, allowing expressions such as `.redirect(dispatcher.root.clone())`.
/// A computed dispatcher expression is evaluated once and borrowed for the
/// registration block; precompute any targets borrowed from it.
///
/// DSL errors point at the offending input. Rust still diagnoses method names,
/// trait bounds, types, and borrowing; import the prelude for trait methods
/// written in your expressions. Unclosed delimiters can be rejected before
/// the macro is invoked. Error wording may change between compiler versions.
#[macro_export]
macro_rules! commands {
    ($($input:tt)*) => {
        $crate::__commands!([$crate]; $($input)*)
    };
}

/// Build a reusable command subtree with the optional `macros` feature.
///
/// Uses the same node syntax and evaluation rules as [`commands!`], but returns
/// the concrete builder so custom fluent methods remain available. A built
/// node with an empty body can also pass through unchanged.
///
/// ```
/// use smaragdine::prelude::*;
/// let subtree = command!(literal("hello") => {
///     run: |_: &CommandContext<()>| -> CommandResult { Ok(1) };
/// }).describe("A reusable command");
/// let mut dispatcher = CommandDispatcher::new();
/// dispatcher.register(subtree);
/// assert_eq!(dispatcher.execute("hello", ()).unwrap(), 1);
/// ```
///
/// `command!(async; node => { ... })` selects async handlers by default and
/// requires the `async` feature. `command!(sync; ...)` explicitly selects sync.
/// This expression has no trailing node `;`; use the normal Rust statement
/// semicolon after the macro invocation when needed.
#[macro_export]
macro_rules! command {
    ($($input:tt)*) => {
        $crate::__command!([$crate]; $($input)*)
    };
}
