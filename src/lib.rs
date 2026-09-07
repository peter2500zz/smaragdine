//! 把 brigadier 指令树变成一个带补全、语法高亮与历史的交互式控制台。
//!
//! ```no_run
//! use smaragdine::prelude::*;
//! use std::sync::Arc;
//!
//! struct App;
//!
//! enum Bye {
//!     Stop,
//!     Restart,
//! }
//!
//! type State = Arc<App>;
//! type Src = Source<State, Bye>;
//!
//! let state = Arc::new(App);
//!
//! let console = Console::<State, Bye>::builder_with_reason()
//!     .command(
//!         literal("echo").describe("把参数原样输出").then(
//!             greedy_string("message")
//!                 .describe("要输出的内容")
//!                 .executes(|ctx: &CommandContext<Src>| -> CommandResult {
//!                     ctx.source.printer().print(get_string(ctx, "message").unwrap_or_default());
//!                     Ok(1)
//!                 }),
//!         ),
//!     )
//!     .command(
//!         literal("stop")
//!             .describe("关停并退出")
//!             .executes(|ctx: &CommandContext<Src>| -> CommandResult {
//!                 ctx.source.request_exit(Bye::Stop);
//!                 Ok(1)
//!             }),
//!     )
//!     .build(Arc::clone(&state));
//!
//! match console.run() {
//!     Exit::Quit(Bye::Restart) => { /* 重新 exec 自己 */ }
//!     Exit::Quit(Bye::Stop) => { /* 优雅关停 */ }
//!     // Ctrl-C / Ctrl-D：没有指令表态，怎么收尾由你定
//!     Exit::Interrupted => {}
//!     // 压根没有可交互的终端（后台服务、容器、输出被重定向）——
//!     // 你的程序必须照跑，改由信号决定何时关停
//!     Exit::NoTerminal => {}
//!     Exit::Failed(e) => eprintln!("控制台读取失败: {e}"),
//! }
//! ```
//!
//! ## 几件要紧的事
//!
//! * **输出一律经 [`Printer`]**。控制台占着终端，别处直接 `println!` 会把
//!   正在编辑的那一行搅乱。
//! * **[`Console::run`] 占住调用它的线程**，直到用户退出。每条指令在自己的
//!   线程上跑，所以慢指令不挡下一条输入。
//! * **指令说明写在节点上**（`describe()`），补全菜单直接读，没有第二张表。
//! * **`requires` 在每一次击键时都会跑**，且拿得到真实状态 —— 只该读廉价
//!   状态，见 [`Source::state`]。

#[cfg(feature = "async")]
mod asynchronous;
mod completer;
mod help;
mod highlighter;
mod history;
mod inspect;
mod interrupt;
mod keys;
mod menu;
mod printer;
mod prompt;
#[cfg(feature = "macros")]
mod registration;
mod source;
#[cfg(test)]
mod testing;
mod text;
mod theme;
mod util;

use std::{io::IsTerminal, sync::Arc};

use azalea_brigadier::{
    builder::IntoCommandNode, command_dispatcher::CommandDispatcher, errors::CommandError,
};
use nu_ansi_term::Style;
use reedline::{EditMode, ExternalPrinter, History, Reedline, ReedlineMenu, Signal};

#[cfg(feature = "async")]
pub use asynchronous::{AsyncConsole, AsyncConsoleBuilder};
pub use help::{Help, Usage, help, usage};
pub use printer::Printer;
pub use source::Source;
pub use text::Text;
pub use theme::{Paint, Piece, Token, default_paint};

// 两个 fork 都得由本库重导出：使用者必须与库用的是同一份 crate，否则
// `Source<S, R>` 与 `CommandDispatcher` 会是两个互不相认的类型。
pub use azalea_brigadier as brigadier;
pub use nu_ansi_term;
pub use reedline;
#[cfg(feature = "macros")]
#[doc(hidden)]
pub use smaragdine_macros::{command as __command, commands as __commands};
#[cfg(feature = "async")]
pub use tokio;

/// 常用的那些东西，外加 brigadier 的建树函数。
pub mod prelude {
    #[cfg(feature = "async")]
    pub use crate::{AsyncConsole, AsyncConsoleBuilder};
    pub use crate::{
        Console, ConsoleBuilder, Exit, Help, Paint, Piece, Printer, Source, Text, Token, Usage,
    };
    #[cfg(feature = "macros")]
    pub use crate::{command, commands};
    pub use azalea_brigadier::prelude::*;
}

/// external printer 的通道容量。
///
/// 满了以后新消息会被丢弃而不是阻塞写入方 —— 写入方可能是业务线程，卡在
/// 那里等于让终端拖垮业务。
const PRINTER_CAPACITY: usize = 256;

const MENU_NAME: &str = "completion_menu";

/// 控制台为何结束。
///
/// `R` 是指令主动退出时携带的原因；不需要原因时保持默认的 `()` 即可。
pub enum Exit<R = ()> {
    /// 某条指令请求了退出，带着它留下的意向。
    Quit(R),
    /// 用户按了 Ctrl-D，或在空行上连按了两次 Ctrl-C。没有指令表态，怎么
    /// 收尾由你定。
    Interrupted,
    /// 压根没有可交互的终端 —— 后台服务、systemd、容器里没分配 tty，或者
    /// 输出被重定向了。
    ///
    /// 必须提前判掉：reedline 在这种环境里会让 `read_line` 立刻返回，若照常
    /// 当成「用户退出」处理，程序会在启动瞬间自己关掉。收到这个就该改由信号
    /// 决定何时关停。
    NoTerminal,
    /// 读取失败。控制台退了，但不是用户的意思 —— 通常也该按「没有终端」
    /// 那样继续跑下去。
    Failed(std::io::Error),
}

/// 指令没跑成时怎么说。
type OnError<S, R> = Arc<dyn Fn(&CommandError, &Source<S, R>) + Send + Sync>;

/// 一个装好了的控制台。
pub struct Console<S, R = ()>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    dispatcher: Arc<CommandDispatcher<Source<S, R>>>,
    source: Source<S, R>,
    printer: Printer,
    text: Text,
    paint: Paint,
    prompt: Option<Box<dyn reedline::Prompt>>,
    indicator: String,
    completion_indicator: String,
    multiline_indicator: String,
    history: Box<dyn History>,
    edit_mode: Box<dyn EditMode>,
    on_error: OnError<S, R>,
}

impl<S, R> Console<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    /// 开始搭一个会携带自定义退出原因的控制台。
    ///
    /// 不需要退出原因时用 [`Console::builder`]，便不必在命令源上多写一个
    /// 泛型参数。
    pub fn builder_with_reason() -> ConsoleBuilder<S, R> {
        ConsoleBuilder::new()
    }

    /// 输出接受器。克隆它，随便带到哪个线程去写。
    pub fn printer(&self) -> Printer {
        self.printer.clone()
    }

    /// 你的应用状态。
    ///
    /// 若交进来的是 `Arc<T>`，这里得到的就是 `&Arc<T>`；需要带走一份时直接
    /// `Arc::clone(console.state())`。
    pub fn state(&self) -> &S {
        self.source.state()
    }

    /// 指令看到的那个源。
    ///
    /// 拿一份留在手边，就能在控制台之外直接执行一行（脚本、测试、远程指令
    /// 通道都用得上）：
    ///
    /// ```
    /// # use smaragdine::prelude::*;
    /// # struct App;
    /// # type Src = Source<App, i32>;
    /// # let console = Console::<App, i32>::builder_with_reason()
    /// #     .command(literal("stop").executes(|ctx: &CommandContext<Src>| -> CommandResult {
    /// #         ctx.source.request_exit(0);
    /// #         Ok(1)
    /// #     }))
    /// #     .build(App);
    /// let dispatcher = console.dispatcher();
    /// dispatcher.execute("stop", console.source()).unwrap();
    /// ```
    pub fn source(&self) -> Source<S, R> {
        self.source.clone()
    }

    /// 指令树。想在控制台之外执行一行时要用到它。
    pub fn dispatcher(&self) -> Arc<CommandDispatcher<Source<S, R>>> {
        Arc::clone(&self.dispatcher)
    }

    /// 跑起来，直到用户退出或确认没有终端可用。
    ///
    /// 占住调用它的线程。期间 [`Printer`] 会被接到 external printer 上；
    /// 返回前恢复成直接写 stdout —— 否则关停阶段的输出会写进一个没人再读的
    /// 通道里。
    pub fn run(self) -> Exit<R> {
        self.run_with(dispatch::<S, R>)
    }

    fn run_with(
        self,
        dispatch_line: impl Fn(
            &Arc<CommandDispatcher<Source<S, R>>>,
            &Source<S, R>,
            &OnError<S, R>,
            &Text,
            &str,
        ),
    ) -> Exit<R> {
        // 后台运行、输出被重定向、容器里没分配 tty —— 这些场景下没有可交互
        // 的终端。必须提前判掉，理由见 Exit::NoTerminal。
        if !std::io::stdin().is_terminal() {
            return Exit::NoTerminal;
        }

        let external = ExternalPrinter::new(PRINTER_CAPACITY);
        self.printer.attach(external.sender());

        let shadow = interrupt::LineShadow::new();
        let armed = interrupt::ExitArmed::new();
        // 输入行右侧那一句：出错原因或此处该填什么。高亮器每次重绘时填，
        // 提示行读。
        let aside = prompt::Aside::new();
        // 菜单里选中第几条，由按键策略维护（见 menu::MenuCursor）。
        let cursor = menu::MenuCursor::new();
        // 「画没画出来」由菜单发布给按键策略：按键往弹窗走还是往历史走，
        // 取决于它。
        let visible = menu::MenuVisible::new();

        let prompt_line = prompt::ConsolePrompt::new(
            aside.clone(),
            self.indicator,
            self.multiline_indicator,
            self.text.clone(),
            Arc::clone(&self.paint),
            self.prompt,
        );

        let mut editor = Reedline::create()
            .with_completer(Box::new(completer::BrigadierCompleter::new(
                Arc::clone(&self.dispatcher),
                self.source.clone(),
            )))
            .with_highlighter(Box::new(highlighter::BrigadierHighlighter::new(
                Arc::clone(&self.dispatcher),
                self.source.clone(),
                self.text.clone(),
                Arc::clone(&self.paint),
                shadow.clone(),
                cursor.clone(),
                aside,
            )))
            .with_hinter(Box::new(interrupt::ExitHint::new(
                armed.clone(),
                self.text.exit_hint.clone(),
                Arc::clone(&self.paint),
            )))
            .with_menu(ReedlineMenu::EngineCompleter(Box::new(
                menu::HidingMenu::new(MENU_NAME, &self.completion_indicator, visible.clone()),
            )))
            // 历史不做前缀过滤，↑↓ 才是纯索引走位（见 history 模块）。
            .with_history(Box::new(history::ConsoleHistory::new(self.history)))
            // 按键策略见 keys 模块。
            .with_edit_mode(Box::new(keys::ConsoleEditMode::new(
                MENU_NAME,
                visible,
                cursor,
                self.edit_mode,
            )))
            .with_external_printer(external)
            // 退出类指令在别的线程上置这个标志，把 read_line 从等待里叫出来。
            .with_break_signal(self.source.interrupt_flag());

        let exit = loop {
            match editor.read_line(&prompt_line) {
                Ok(Signal::Success(line)) => {
                    armed.disarm();
                    if !line.trim().is_empty() {
                        dispatch_line(
                            &self.dispatcher,
                            &self.source,
                            &self.on_error,
                            &self.text,
                            &line,
                        );
                    }
                }
                // 终端处于 raw 模式，Ctrl-C 是当按键送进来的（不会变成
                // SIGINT），所以只有这里能接。行里有内容时它只抹掉内容，
                // 空行上连按两次才退出。
                Ok(Signal::CtrlC) => match interrupt::on_interrupt(&shadow, &armed) {
                    interrupt::Interrupt::Exit => break Exit::Interrupted,
                    interrupt::Interrupt::Armed | interrupt::Interrupt::ClearedLine => continue,
                },
                Ok(Signal::CtrlD) => break Exit::Interrupted,
                // 退出类指令在别的线程上置了中断标志，把我们从等待里放了
                // 出来 —— 下面统一读退出意向。
                Ok(_) => {}
                Err(e) => break Exit::Failed(e),
            }

            // 指令跑在别的线程上，够不着这个循环 —— 只能留个退出意向。这里
            // 统一读：无论是刚提交完一行，还是被退出指令的中断标志叫醒，都
            // 走这一处。
            if let Some(exit) = self.source.take_exit() {
                break Exit::Quit(exit);
            }
        };

        // 摘掉 external printer：控制台已经退了，通道那头没人读了。
        self.printer.detach();
        exit
    }
}

impl<S> Console<S>
where
    S: Send + Sync + 'static,
{
    /// 开始搭一个控制台。
    ///
    /// 状态类型由最后的 [`ConsoleBuilder::build`] 推导；它可以是 `Arc<T>`、
    /// 自定义包装或任何满足线程安全约束的类型，不需要实现库 trait。
    ///
    /// ```
    /// use smaragdine::Console;
    /// use std::sync::Arc;
    ///
    /// struct App;
    /// let state = Arc::new(App);
    /// let console = Console::builder().build(Arc::clone(&state));
    ///
    /// assert!(Arc::ptr_eq(console.state(), &state));
    /// ```
    pub fn builder() -> ConsoleBuilder<S> {
        ConsoleBuilder::new()
    }
}

/// 把一行指令投出去执行。
///
/// 每条指令一个线程，控制台立刻回到 `read_line` —— 于是上一条不拦下一条，
/// 想连着发多少条都行。而 external printer 恰恰只在 `read_line` 跑着的时候
/// 排空，正是指令在后台跑的那段时间。
///
/// 用系统线程而不是异步任务：指令体是同步代码，想等一个网络请求就得
/// `block_on`，那在运行时线程里会 panic，在普通线程里才是合法用法。
fn dispatch<S, R>(
    dispatcher: &Arc<CommandDispatcher<Source<S, R>>>,
    source: &Source<S, R>,
    on_error: &OnError<S, R>,
    text: &Text,
    line: &str,
) where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let spawned = {
        let (dispatcher, source, on_error, text, line) = (
            Arc::clone(dispatcher),
            source.clone(),
            Arc::clone(on_error),
            text.clone(),
            line.to_owned(),
        );
        std::thread::Builder::new()
            .name("command".to_owned())
            .spawn(move || execute(&dispatcher, &source, &on_error, &text, &line))
    };

    if spawned.is_err() {
        // 起不了线程就退回同步执行 —— 指令没跑总比悄悄丢掉强，但得说一声：
        // 这条指令跑完之前，输入是没反应的。
        source.printer().print(&text.command_ran_inline);
        execute(dispatcher, source, on_error, text, line);
    }
}

/// 真正执行一行指令。
///
/// 包在 `catch_unwind` 里：指令体是普通 Rust 代码，谁都可能写出 panic，
/// 但一条指令写崩了不该把整个程序带走 —— 何况它现在跑在自己的线程上。
///
/// 于是使用方**不能**设 `panic = "abort"`。
fn execute<S, R>(
    dispatcher: &CommandDispatcher<Source<S, R>>,
    source: &Source<S, R>,
    on_error: &OnError<S, R>,
    text: &Text,
    line: &str,
) where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatcher.execute(line, source.clone())
    }));

    match outcome {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => on_error(&e, source),
        Err(_) => source.printer().print(&text.command_panicked),
    }
}

/// 搭控制台。
///
/// `S` 是调用方原样交进来的状态；`R` 是可选的退出原因，默认 `()`。状态的
/// 包装形状不属于本库 API，只要满足跨线程所需的约束即可。
pub struct ConsoleBuilder<S, R = ()>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    dispatcher: CommandDispatcher<Source<S, R>>,
    printer: Option<Printer>,
    text: Text,
    paint: Paint,
    prompt: Option<Box<dyn reedline::Prompt>>,
    indicator: String,
    completion_indicator: String,
    multiline_indicator: String,
    history: Option<Box<dyn History>>,
    edit_mode: Option<Box<dyn EditMode>>,
    on_error: Option<OnError<S, R>>,
}

impl<S, R> Default for ConsoleBuilder<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S, R> ConsoleBuilder<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    pub fn new() -> Self {
        Self {
            dispatcher: CommandDispatcher::new(),
            printer: None,
            text: Text::default(),
            paint: Arc::new(default_paint),
            prompt: None,
            indicator: "> ".to_owned(),
            completion_indicator: "| ".to_owned(),
            multiline_indicator: "| ".to_owned(),
            history: None,
            edit_mode: None,
            on_error: None,
        }
    }

    /// 挂一条指令。
    ///
    /// 说明写在节点上（`describe()`），补全菜单直接读它 —— 没有第二张表要
    /// 维护，同名子指令（`proxy on` 与 `log on`）也各说各的。
    ///
    /// 接受保留具体解析器类型的 builder，也接受已经构建的 `CommandNode`。
    /// 自定义 builder 实现 `CommandBuilder` 即可；其他命令包装类型实现
    /// `IntoCommandNode`，内置 [`Help`] 已适配。
    ///
    /// ```
    /// use smaragdine::prelude::*;
    /// let _console = Console::builder()
    ///     .command(integer("count").describe("次数").range(1..=10)
    ///         .executes(|ctx: &CommandContext<Source<()>>| -> CommandResult {
    ///             Ok(get_integer(ctx, "count").unwrap())
    ///         }))
    ///     .build(());
    /// ```
    pub fn command(mut self, command: impl IntoCommandNode<Source<S, R>, i32>) -> Self {
        self.dispatcher.register(command);
        self
    }

    /// 直接摆布指令树。
    ///
    /// 重定向、fork、自定义参数类型这些 [`Self::command`] 表达不了的用法走
    /// 这里 —— brigadier 的全部能力都在，库不挡道。
    pub fn commands(mut self, build: impl FnOnce(&mut CommandDispatcher<Source<S, R>>)) -> Self {
        build(&mut self.dispatcher);
        self
    }

    /// 用这个接受器，而不是新建一个。
    ///
    /// 日志通常先于控制台起来，那就先建好接受器接上日志，再交给这里。
    pub fn printer(mut self, printer: Printer) -> Self {
        self.printer = Some(printer);
        self
    }

    /// 换掉提示符那几个字（默认 `"> "`）。
    pub fn prompt(mut self, indicator: impl Into<String>) -> Self {
        self.indicator = indicator.into();
        self
    }

    /// 换掉补全菜单可见时临时顶替主提示符的符号（默认 `"| "`）。
    ///
    /// 它与 [`Self::multiline_prompt`] 是两处独立的提示符：前者跟着补全菜单
    /// 显示，后者只出现在输入缓冲区的显式换行之后。传空字符串即可让补全
    /// 菜单显示时不画这个符号。
    ///
    /// ```
    /// # use smaragdine::Console;
    /// # struct App;
    /// let console = Console::builder()
    ///     .completion_prompt("/ ")
    ///     .build(App);
    /// # let _ = console;
    /// ```
    pub fn completion_prompt(mut self, indicator: impl Into<String>) -> Self {
        self.completion_indicator = indicator.into();
        self
    }

    /// 换掉显式换行后每一行开头的提示符（默认 `"| "`）。
    ///
    /// 传空字符串即可不画多行提示符。若同时使用 [`Self::prompt_with`]，整个
    /// 提示符都由自定义 [`reedline::Prompt`] 接管，这一项便不再生效。
    ///
    /// ```
    /// # use smaragdine::Console;
    /// # struct App;
    /// let console = Console::builder()
    ///     .multiline_prompt("... ")
    ///     .build(App);
    /// # let _ = console;
    /// ```
    pub fn multiline_prompt(mut self, indicator: impl Into<String>) -> Self {
        self.multiline_indicator = indicator.into();
        self
    }

    /// 整个换掉提示符。
    ///
    /// 除右侧那一句之外一律听它的 —— 右侧只在控制台有话要说（出错原因、
    /// 此处该填什么）时才被抢过去。
    pub fn prompt_with(mut self, prompt: Box<dyn reedline::Prompt>) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// 换掉着色方案。
    ///
    /// 每次重绘都会对每一小段调用一遍，别在里面做重活。
    pub fn paint(mut self, paint: impl Fn(&Token) -> Style + Send + Sync + 'static) -> Self {
        self.paint = Arc::new(paint);
        self
    }

    /// 换掉控制台自己会说的那几句话。
    pub fn text(mut self, text: Text) -> Self {
        self.text = text;
        self
    }

    /// 换掉历史的存储。
    ///
    /// 默认只在内存里。想跨会话保留就给一个文件历史：
    ///
    /// ```no_run
    /// # use smaragdine::prelude::*;
    /// # struct App;
    /// use smaragdine::reedline::FileBackedHistory;
    ///
    /// let history = FileBackedHistory::with_file(1000, "history.txt".into())?;
    /// let console = Console::<App>::builder().history(Box::new(history)).build(App);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// 无论换成什么，↑↓ 都仍是纯索引走位 —— reedline 默认的前缀过滤在库里
    /// 摘掉了，理由见 `history` 模块。
    pub fn history(mut self, history: Box<dyn History>) -> Self {
        self.history = Some(history);
        self
    }

    /// 换掉底层键位表。
    ///
    /// 默认以 reedline 的 Emacs 键位为底，但不启用 Ctrl-L/R/O/B/F/P/N。显式
    /// 交进来的编辑模式不受这项默认策略限制。
    ///
    /// 补全弹窗独占的那几个键（↑↓ / Tab / Shift+Tab / Esc / 回车）仍由库
    /// 接管 —— 弹窗不要的键才轮到这一层。
    pub fn edit_mode(mut self, edit_mode: Box<dyn EditMode>) -> Self {
        self.edit_mode = Some(edit_mode);
        self
    }

    /// 指令没跑成时怎么说。
    ///
    /// 默认把 brigadier 的英文原文打出来。语法错误仍可通过
    /// `err.syntax().map(|syntax| syntax.kind())` 取得带值的枚举，所以想换措辞、
    /// 换语言、或者干脆记进日志，都在这里做：
    ///
    /// ```
    /// # use smaragdine::prelude::*;
    /// # struct App;
    /// use smaragdine::brigadier::errors::BuiltInError;
    ///
    /// let console = Console::<App>::builder()
    ///     .on_error(|err, source| {
    ///         source.printer().print(match err.syntax().map(|syntax| syntax.kind()) {
    ///             Some(BuiltInError::DispatcherUnknownCommand) => "不认识的指令".to_owned(),
    ///             _ => err.message(),
    ///         });
    ///     })
    ///     .build(App);
    /// ```
    pub fn on_error(
        mut self,
        on_error: impl Fn(&CommandError, &Source<S, R>) + Send + Sync + 'static,
    ) -> Self {
        self.on_error = Some(Arc::new(on_error));
        self
    }

    /// 装好，交出应用状态。
    pub fn build(self, state: S) -> Console<S, R> {
        let printer = self.printer.unwrap_or_default();

        Console {
            dispatcher: Arc::new(self.dispatcher),
            source: Source::with_printer(state, printer.clone()),
            printer,
            text: self.text,
            paint: self.paint,
            prompt: self.prompt,
            indicator: self.indicator,
            completion_indicator: self.completion_indicator,
            multiline_indicator: self.multiline_indicator,
            history: self
                .history
                .unwrap_or_else(|| Box::new(reedline::FileBackedHistory::default())),
            edit_mode: self
                .edit_mode
                .unwrap_or_else(|| Box::new(keys::default_edit_mode())),
            on_error: self.on_error.unwrap_or_else(|| {
                Arc::new(|e, source: &Source<S, R>| source.printer().print(e.message()))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Nothing;
    use azalea_brigadier::prelude::*;
    use std::sync::{
        Arc as StdArc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Wrapped<T>(T);

    #[test]
    fn console_accepts_typed_arguments_built_nodes_and_command_wrappers() {
        type Src = Source<Nothing>;
        let errors = StdArc::new(AtomicUsize::new(0));
        let builtin = literal::<Src, i32>("ready")
            .executes(|_| -> CommandResult { Ok(1) })
            .build();
        let console = Console::builder()
            .command(
                integer("count")
                    .describe("次数")
                    .executes(|ctx: &CommandContext<Src>| -> CommandResult {
                        Ok(get_integer(ctx, "count").unwrap())
                    })
                    .range(1..=3),
            )
            .command(builtin)
            .command(help("help"))
            .on_error({
                let errors = StdArc::clone(&errors);
                move |error, _| {
                    assert!(matches!(
                        error.syntax().unwrap().kind(),
                        azalea_brigadier::errors::BuiltInError::IntegerTooBig { .. }
                    ));
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            })
            .build(Nothing { unlocked: true });

        assert_eq!(
            console.dispatcher.execute("2 ", console.source()).unwrap(),
            2
        );
        assert_eq!(
            console
                .dispatcher
                .execute("ready", console.source())
                .unwrap(),
            1
        );
        assert!(console.dispatcher.root.read().child("help").is_some());
        run_line(&console, "4");
        assert_eq!(errors.load(Ordering::Relaxed), 1);
    }

    fn console() -> Console<Nothing, i32> {
        Console::<Nothing, i32>::builder_with_reason()
            .command(literal("quit").describe("退出").executes(
                |ctx: &CommandContext<Source<Nothing, i32>>| -> CommandResult {
                    ctx.source.request_exit(3);
                    Ok(1)
                },
            ))
            .command(literal("boom").executes(
                |_: &CommandContext<Source<Nothing, i32>>| -> CommandResult {
                    panic!("指令体崩了");
                },
            ))
            .build(Nothing { unlocked: true })
    }

    fn run_line<R: Send + 'static>(console: &Console<Nothing, R>, line: &str) {
        execute(
            &console.dispatcher,
            &console.source,
            &console.on_error,
            &console.text,
            line,
        );
    }

    /// 指令执行必须吃得下任何输入而不 panic —— 它直接连着用户键盘。
    ///
    /// 这里走的是同步的 `execute` 而不是另起线程的 `dispatch`：要验的是指令
    /// 体与解析器扛不扛得住，线程投递那一层没有可断言的东西。
    #[test]
    fn executing_survives_anything() {
        let console = console();
        for line in [
            "quit",
            "nope",
            "你好",
            "🎮",
            "   ",
            "\\",
            "quit \\",
            "quit \"带引号 的内容\"",
        ] {
            run_line(&console, line);
        }
    }

    /// 指令体 panic 不该把程序带走 —— 兜住，并说一声。
    #[test]
    fn a_panicking_command_is_caught() {
        let console = console();
        let printer = console.printer();
        let (sender, receiver) = crossbeam::channel::bounded(4);
        printer.attach(sender);

        run_line(&console, "boom");

        assert_eq!(
            receiver.try_recv().as_deref(),
            Ok(Text::default().command_panicked.as_str())
        );
    }

    /// 指令留下的退出意向要回得到主循环。
    #[test]
    fn an_exit_request_reaches_the_loop() {
        let console = console();
        assert!(console.source.take_exit().is_none());

        run_line(&console, "quit");
        assert_eq!(console.source.take_exit(), Some(3));
    }

    /// 别的指令不能顺手留下退出意向。
    #[test]
    fn other_commands_do_not_request_an_exit() {
        let console = console();
        for line in ["nope", "boom", "你好"] {
            run_line(&console, line);
        }
        assert!(console.source.take_exit().is_none());
    }

    /// 指令没跑成时，措辞归使用者。
    #[test]
    fn the_error_hook_gets_the_structured_error() {
        let seen = StdArc::new(AtomicUsize::new(0));
        let console = Console::builder()
            .command(
                literal("quit")
                    .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
            )
            .on_error({
                let seen = StdArc::clone(&seen);
                move |err, source| {
                    seen.fetch_add(1, Ordering::Relaxed);
                    // 结构化的，够得着 kind 与原文。
                    assert!(!err.message().is_empty());
                    source.printer().print("换个说法");
                }
            })
            .build(Nothing { unlocked: true });

        run_line(&console, "nope");
        assert_eq!(seen.load(Ordering::Relaxed), 1);

        run_line(&console, "quit");
        assert_eq!(seen.load(Ordering::Relaxed), 1, "跑成了就不该报错");
    }

    #[test]
    fn the_error_hook_gets_execution_errors() {
        let seen = StdArc::new(AtomicUsize::new(0));
        let console = Console::builder()
            .command(literal("fail").executes(
                |_: &CommandContext<Source<Nothing>>| -> Result<i32, std::io::Error> {
                    Err(std::io::Error::other("command failed"))
                },
            ))
            .on_error({
                let seen = StdArc::clone(&seen);
                move |err, _| {
                    seen.fetch_add(1, Ordering::Relaxed);
                    assert!(err.execution().is_some());
                    assert!(err.syntax().is_none());
                    assert_eq!(err.to_string(), "command failed");
                }
            })
            .build(Nothing { unlocked: true });

        run_line(&console, "fail");
        assert_eq!(seen.load(Ordering::Relaxed), 1);
    }

    /// 没有终端时不能当成「用户退出」—— 否则程序会在启动瞬间自己关掉。
    ///
    /// 测试环境里 stdin 不是终端，正好是这条路。
    #[test]
    fn running_without_a_terminal_says_so() {
        assert!(matches!(console().run(), Exit::NoTerminal));
    }

    /// 状态与接受器在装好之后都还够得着。
    #[test]
    fn the_console_hands_back_what_you_gave_it() {
        let console = console();
        assert!(console.state().unlocked);
        assert!(
            console
                .dispatcher()
                .execute("quit", console.source())
                .is_ok()
        );
    }

    /// Builder 接受调用方原有的共享状态，不要求为某种包装形状实现库 trait。
    #[test]
    fn the_builder_accepts_an_existing_arc_state() {
        let state = StdArc::new(Nothing { unlocked: true });
        let console: Console<StdArc<Nothing>> = Console::builder().build(StdArc::clone(&state));

        assert!(StdArc::ptr_eq(console.state(), &state));
    }

    #[test]
    fn the_builder_accepts_an_arbitrary_state_wrapper() {
        let state = StdArc::new(Nothing { unlocked: true });
        let console: Console<Wrapped<StdArc<Nothing>>> =
            Console::builder().build(Wrapped(StdArc::clone(&state)));

        assert!(StdArc::ptr_eq(&console.state().0, &state));
    }

    #[test]
    fn completion_and_multiline_prompts_are_independent() {
        let defaults = Console::builder().build(Nothing { unlocked: true });
        assert_eq!(defaults.completion_indicator, "| ");
        assert_eq!(defaults.multiline_indicator, "| ");

        let configured = Console::builder()
            .completion_prompt("menu> ")
            .multiline_prompt("line> ")
            .build(Nothing { unlocked: true });
        assert_eq!(configured.completion_indicator, "menu> ");
        assert_eq!(configured.multiline_indicator, "line> ");
    }
}
