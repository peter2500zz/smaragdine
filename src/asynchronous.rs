//! Tokio-task-backed console execution.

use std::sync::Arc;

use azalea_brigadier::{
    builder::IntoCommandNode, command_dispatcher::CommandDispatcher, errors::CommandError,
};
use nu_ansi_term::Style;
use reedline::{EditMode, History};
use tokio::runtime::Handle;

use crate::{Console, ConsoleBuilder, Exit, OnError, Printer, Source, Text, Token};

/// A console that submits each command to a caller-owned Tokio runtime.
///
/// [`Self::run`] still blocks the thread that owns the terminal. It does not
/// create a runtime or a command thread: command parsing and execution happen
/// in Tokio tasks spawned through the [`Handle`] supplied to the builder.
/// Keep that runtime alive and driven for as long as commands may still run.
/// In particular, do not block the only driver of a current-thread runtime in
/// [`Self::run`].
pub struct AsyncConsole<S, R = ()>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    console: Console<S, R>,
    runtime: Handle,
}

impl<S, R> AsyncConsole<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    /// Start building a console with a custom exit reason.
    pub fn builder_with_reason(runtime: Handle) -> AsyncConsoleBuilder<S, R> {
        AsyncConsoleBuilder::new(runtime)
    }

    /// The runtime handle used for command tasks.
    pub fn runtime(&self) -> &Handle {
        &self.runtime
    }

    /// Output receiver shared by the console and its commands.
    pub fn printer(&self) -> Printer {
        self.console.printer()
    }

    /// The application state supplied to the builder.
    pub fn state(&self) -> &S {
        self.console.state()
    }

    /// The source passed to commands.
    pub fn source(&self) -> Source<S, R> {
        self.console.source()
    }

    /// The command tree.
    pub fn dispatcher(&self) -> Arc<CommandDispatcher<Source<S, R>>> {
        self.console.dispatcher()
    }

    /// Run until the user exits or no interactive terminal is available.
    ///
    /// This occupies the calling thread, just like [`Console::run`]. Each
    /// submitted line is detached onto the configured Tokio runtime. Pending
    /// commands may therefore outlive this method; their lifetime is governed
    /// by the caller-owned runtime.
    pub fn run(self) -> Exit<R> {
        let Self { console, runtime } = self;
        console.run_with(|dispatcher, source, on_error, text, line| {
            dispatch_async(&runtime, dispatcher, source, on_error, text, line);
        })
    }
}

impl<S> AsyncConsole<S>
where
    S: Send + Sync + 'static,
{
    /// Start building an async console.
    pub fn builder(runtime: Handle) -> AsyncConsoleBuilder<S> {
        AsyncConsoleBuilder::new(runtime)
    }
}

/// Builder for [`AsyncConsole`].
///
/// It deliberately wraps the synchronous builder rather than replacing it,
/// so enabling the Cargo feature cannot alter [`Console`] or
/// [`ConsoleBuilder`] semantics.
pub struct AsyncConsoleBuilder<S, R = ()>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    inner: ConsoleBuilder<S, R>,
    runtime: Handle,
}

impl<S, R> AsyncConsoleBuilder<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    /// Create a builder that submits commands through `runtime`.
    pub fn new(runtime: Handle) -> Self {
        Self {
            inner: ConsoleBuilder::new(),
            runtime,
        }
    }

    /// Register a command tree branch.
    ///
    /// Both `executes_async` and existing `executes` actions are accepted.
    /// Typed argument builders and built command nodes are both accepted;
    /// numeric bounds remain configurable after attaching an async handler.
    /// Synchronous actions run directly on a Tokio worker, so they must remain
    /// short and non-blocking; waiting work belongs in `executes_async`.
    pub fn command(mut self, command: impl IntoCommandNode<Source<S, R>, i32>) -> Self {
        self.inner = self.inner.command(command);
        self
    }

    /// Directly mutate the underlying Brigadier dispatcher.
    pub fn commands(mut self, build: impl FnOnce(&mut CommandDispatcher<Source<S, R>>)) -> Self {
        self.inner = self.inner.commands(build);
        self
    }

    /// Use an existing output receiver.
    pub fn printer(mut self, printer: Printer) -> Self {
        self.inner = self.inner.printer(printer);
        self
    }

    /// Configure the main prompt indicator.
    pub fn prompt(mut self, indicator: impl Into<String>) -> Self {
        self.inner = self.inner.prompt(indicator);
        self
    }

    /// Configure the prompt shown while the completion menu is visible.
    pub fn completion_prompt(mut self, indicator: impl Into<String>) -> Self {
        self.inner = self.inner.completion_prompt(indicator);
        self
    }

    /// Configure the continuation prompt for explicit newlines.
    pub fn multiline_prompt(mut self, indicator: impl Into<String>) -> Self {
        self.inner = self.inner.multiline_prompt(indicator);
        self
    }

    /// Replace the prompt implementation.
    pub fn prompt_with(mut self, prompt: Box<dyn reedline::Prompt>) -> Self {
        self.inner = self.inner.prompt_with(prompt);
        self
    }

    /// Replace the syntax-colouring function.
    pub fn paint(mut self, paint: impl Fn(&Token) -> Style + Send + Sync + 'static) -> Self {
        self.inner = self.inner.paint(paint);
        self
    }

    /// Replace the console's text table.
    pub fn text(mut self, text: Text) -> Self {
        self.inner = self.inner.text(text);
        self
    }

    /// Replace the history backend.
    pub fn history(mut self, history: Box<dyn History>) -> Self {
        self.inner = self.inner.history(history);
        self
    }

    /// Replace the underlying edit mode.
    pub fn edit_mode(mut self, edit_mode: Box<dyn EditMode>) -> Self {
        self.inner = self.inner.edit_mode(edit_mode);
        self
    }

    /// Handle structured command errors.
    pub fn on_error(
        mut self,
        on_error: impl Fn(&CommandError, &Source<S, R>) + Send + Sync + 'static,
    ) -> Self {
        self.inner = self.inner.on_error(on_error);
        self
    }

    /// Build the console around `state`.
    pub fn build(self, state: S) -> AsyncConsole<S, R> {
        AsyncConsole {
            console: self.inner.build(state),
            runtime: self.runtime,
        }
    }
}

/// Submit a line without creating a command-specific operating-system thread.
///
/// The outer task observes the inner command task so a panic is reported with
/// the same user-facing text as the synchronous console. Both tasks are
/// lightweight Tokio tasks; dropping their handles detaches them, matching the
/// existing console's fire-and-forget command lifetime.
fn dispatch_async<S, R>(
    runtime: &Handle,
    dispatcher: &Arc<CommandDispatcher<Source<S, R>>>,
    source: &Source<S, R>,
    on_error: &OnError<S, R>,
    text: &Text,
    line: &str,
) where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let command = {
        let dispatcher = Arc::clone(dispatcher);
        let source = source.clone();
        let line = line.to_owned();
        runtime.spawn(async move { dispatcher.execute_async(line, source).await })
    };

    let source = source.clone();
    let on_error = Arc::clone(on_error);
    let text = text.clone();
    runtime.spawn(async move {
        match command.await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => on_error(&error, &source),
            Err(error) if error.is_panic() => {
                source.printer().print(&text.command_panicked);
            }
            // A command is cancelled only when its caller-owned runtime is
            // shutting down (the console keeps no abort handle). At that point
            // there is no executor left on which to report it reliably.
            Err(_) => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
    };

    use azalea_brigadier::prelude::*;
    use tokio::{runtime::Builder, task::yield_now};

    use super::*;
    use crate::testing::Nothing;

    async fn read_state(ctx: Arc<CommandContext<Source<Nothing>>>) -> CommandResult {
        Ok(i32::from(ctx.source.state().unlocked))
    }

    fn drive_until(runtime: &tokio::runtime::Runtime, done: impl Fn() -> bool) {
        runtime.block_on(async {
            for _ in 0..20_000 {
                if done() {
                    return;
                }
                yield_now().await;
            }
            panic!("Tokio tasks did not finish");
        });
    }

    #[test]
    fn builder_keeps_the_runtime_and_state() {
        let runtime = Builder::new_current_thread().build().unwrap();
        let state = Arc::new(Nothing { unlocked: true });
        let console = AsyncConsole::builder(runtime.handle().clone()).build(Arc::clone(&state));

        assert!(Arc::ptr_eq(console.state(), &state));
        assert_eq!(console.runtime().id(), runtime.handle().id());
    }

    #[test]
    fn builder_accepts_a_named_async_function_directly() {
        let runtime = Builder::new_current_thread().build().unwrap();
        let console = AsyncConsole::builder(runtime.handle().clone())
            .command(literal("state").executes_async(read_state))
            .build(Nothing { unlocked: true });

        let dispatcher = console.dispatcher();
        let result = runtime
            .block_on(dispatcher.execute_async("state", console.source()))
            .unwrap();

        assert_eq!(result, 1);
    }

    #[test]
    fn builder_accepts_typed_argument_roots_and_built_nodes() {
        async fn read_count(ctx: Arc<CommandContext<Source<Nothing>>>) -> CommandResult {
            yield_now().await;
            Ok(get_integer(&ctx, "count").unwrap())
        }
        let runtime = Builder::new_current_thread().build().unwrap();
        let console = AsyncConsole::builder(runtime.handle().clone())
            .command(integer("count").executes_async(read_count).range(1..=3))
            .command(literal("state").executes_async(read_state).build())
            .command(crate::help("help"))
            .build(Nothing { unlocked: true });
        runtime.block_on(async {
            let dispatcher = console.dispatcher();
            assert_eq!(
                dispatcher
                    .execute_async("2 ", console.source())
                    .await
                    .unwrap(),
                2
            );
            assert!(
                dispatcher
                    .execute_async("4", console.source())
                    .await
                    .unwrap_err()
                    .syntax()
                    .is_some()
            );
            assert_eq!(
                dispatcher
                    .execute_async("state", console.source())
                    .await
                    .unwrap(),
                1
            );
            assert!(dispatcher.root.read().child("help").is_some());
        });
    }

    #[test]
    fn async_commands_run_on_tokio_not_command_threads() {
        const COMMANDS: usize = 10_000;

        let runtime = Builder::new_current_thread().build().unwrap();
        let (sender, receiver) = mpsc::channel();
        let completed = Arc::new(AtomicUsize::new(0));
        let console = AsyncConsole::builder(runtime.handle().clone())
            .command(literal("where").executes_async({
                let completed = Arc::clone(&completed);
                move |_: Arc<CommandContext<Source<Nothing>>>| {
                    let sender = sender.clone();
                    let completed = Arc::clone(&completed);
                    async move {
                        sender.send(thread::current().id()).unwrap();
                        completed.fetch_add(1, Ordering::Relaxed);
                        Ok::<_, BoxCommandError>(1)
                    }
                }
            }))
            .build(Nothing { unlocked: true });

        for _ in 0..COMMANDS {
            dispatch_async(
                console.runtime(),
                &console.console.dispatcher,
                &console.console.source,
                &console.console.on_error,
                &console.console.text,
                "where",
            );
        }

        drive_until(&runtime, || completed.load(Ordering::Relaxed) == COMMANDS);
        let task_threads = receiver.try_iter().collect::<Vec<_>>();
        assert_eq!(task_threads.len(), COMMANDS);
        assert!(
            task_threads
                .iter()
                .all(|thread_id| *thread_id == thread::current().id())
        );
    }

    #[test]
    fn a_waiting_task_does_not_block_the_next_command() {
        let runtime = Builder::new_current_thread().build().unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicUsize::new(0));
        let console = AsyncConsole::builder(runtime.handle().clone())
            .command(literal("slow").executes_async({
                let released = Arc::clone(&released);
                let completed = Arc::clone(&completed);
                move |_: Arc<CommandContext<Source<Nothing>>>| {
                    let released = Arc::clone(&released);
                    let completed = Arc::clone(&completed);
                    async move {
                        while !released.load(Ordering::Acquire) {
                            yield_now().await;
                        }
                        completed.fetch_add(1, Ordering::Relaxed);
                        Ok::<_, BoxCommandError>(1)
                    }
                }
            }))
            .command(literal("fast").executes_async({
                let released = Arc::clone(&released);
                let completed = Arc::clone(&completed);
                move |_: Arc<CommandContext<Source<Nothing>>>| {
                    let released = Arc::clone(&released);
                    let completed = Arc::clone(&completed);
                    async move {
                        released.store(true, Ordering::Release);
                        completed.fetch_add(1, Ordering::Relaxed);
                        Ok::<_, BoxCommandError>(1)
                    }
                }
            }))
            .build(Nothing { unlocked: true });

        for line in ["slow", "fast"] {
            dispatch_async(
                console.runtime(),
                &console.console.dispatcher,
                &console.console.source,
                &console.console.on_error,
                &console.console.text,
                line,
            );
        }

        drive_until(&runtime, || completed.load(Ordering::Relaxed) == 2);
    }

    #[test]
    fn panics_and_syntax_errors_are_reported() {
        async fn panic_command(_: Arc<CommandContext<Source<Nothing>>>) -> CommandResult {
            panic!("test panic");
        }

        let runtime = Builder::new_current_thread().build().unwrap();
        let errors = Arc::new(AtomicUsize::new(0));
        let console = AsyncConsole::builder(runtime.handle().clone())
            .command(literal("boom").executes_async(panic_command))
            .on_error({
                let errors = Arc::clone(&errors);
                move |_, _| {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            })
            .build(Nothing { unlocked: true });
        let (sender, receiver) = crossbeam::channel::bounded(1);
        console.printer().attach(sender);

        for line in ["boom", "missing"] {
            dispatch_async(
                console.runtime(),
                &console.console.dispatcher,
                &console.console.source,
                &console.console.on_error,
                &console.console.text,
                line,
            );
        }

        drive_until(&runtime, || {
            errors.load(Ordering::Relaxed) == 1 && !receiver.is_empty()
        });
        assert_eq!(
            receiver.try_recv().as_deref(),
            Ok(Text::default().command_panicked.as_str())
        );
    }

    #[test]
    fn the_synchronous_console_keeps_its_thread_mode_with_the_feature_enabled() {
        let (sender, receiver) = mpsc::channel();
        let console = Console::builder()
            .command(literal("where").executes(
                move |_: &CommandContext<Source<Nothing>>| -> CommandResult {
                    sender.send(thread::current().id()).unwrap();
                    Ok(1)
                },
            ))
            .build(Nothing { unlocked: true });

        crate::dispatch(
            &console.dispatcher,
            &console.source,
            &console.on_error,
            &console.text,
            "where",
        );

        assert_ne!(receiver.recv().unwrap(), thread::current().id());
    }

    #[test]
    fn running_without_a_terminal_has_the_same_exit_reason() {
        let runtime = Builder::new_current_thread().build().unwrap();
        let console =
            AsyncConsole::builder(runtime.handle().clone()).build(Nothing { unlocked: true });

        assert!(matches!(console.run(), Exit::NoTerminal));
    }
}
