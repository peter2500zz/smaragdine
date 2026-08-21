//! 一个能跑的小控制台，把库的各处都用上一遍。
//!
//! ```shell
//! cargo run --example demo
//! ```
//!
//! 试试这些：
//!
//! * 打 `p`，看补全菜单与右边的说明；Tab 采用，再按 Tab 在候选间循环
//! * 打 `proxy `，再打 `log `，两个 `on` 各说各的
//! * 打 `echo`，光标处会浮出 `<message>`，右侧告诉你它是什么
//! * 打 `echo hi 多余的`，看它变红、右侧报参数不对
//! * `danger` 一开始不在菜单里 —— `unlock` 之后才出现
//! * 后台每三秒打一行，它落在提示行**上方**，不会搅乱你正在输入的内容
//! * 空行上按 Ctrl-C，行尾浮出「再按一次退出」
//! * `help` 列顶层，`help log` 往下看一层 —— 路径也能 Tab 补全

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use smaragdine::{Text, prelude::*};

/// 交给指令使用的东西。
///
/// 指令跑在各自的线程上，所以状态必须能并发读写；整份状态由调用方自己的
/// `Arc` 共享，控制台、HTTP 服务和后台任务都可以拿同一个句柄。
struct App {
    proxy: AtomicBool,
    verbose: AtomicBool,
    unlocked: AtomicBool,
    ticks: AtomicU64,
}

/// 退出时想让主程序做什么。库不解释它的含义，只负责原样交还。
enum Bye {
    Stop,
    Restart,
}

/// 指令树里到处要写它，取个短名字。
type State = Arc<App>;
type Src = Source<State, Bye>;

fn main() {
    let state = Arc::new(App {
        proxy: AtomicBool::new(false),
        verbose: AtomicBool::new(false),
        unlocked: AtomicBool::new(false),
        ticks: AtomicU64::new(0),
    });

    let console = Console::<State, Bye>::builder_with_reason()
        // 库自己会说的那几句话，换成中文。
        .text(Text {
            exit_hint: "  再按一次 Ctrl-C 退出".to_owned(),
            unknown_command: "不认识的指令，或还没打完".to_owned(),
            incorrect_argument: "参数不对".to_owned(),
            history_search: "回溯".to_owned(),
            history_search_failing: "没找到 ".to_owned(),
            ..Text::default()
        })
        .prompt("demo> ")
        // 粘贴或 Shift+Enter 产生显式换行时，每个后续行从这里开始。
        .multiline_prompt("... ")
        // 指令没跑成时的措辞。brigadier 的错误是结构化的，想换哪句换哪句。
        .on_error(|err, source| {
            use smaragdine::brigadier::errors::BuiltInError;

            source.printer().print(match err.kind() {
                BuiltInError::DispatcherUnknownCommand => {
                    "没有这条指令，打 help 看看有什么".to_owned()
                }
                _ => err.message(),
            });
        })
        .command(
            literal("echo").describe("把参数原样输出").then(
                argument("message", greedy_string())
                    .describe("要输出的内容")
                    .executes(|ctx: &CommandContext<Src>| {
                        ctx.source
                            .printer()
                            .print(get_string(ctx, "message").unwrap_or_default());
                        1
                    }),
            ),
        )
        .command(literal("status").describe("看看现在是什么状态").executes(
            |ctx: &CommandContext<Src>| {
                let app = ctx.source.state();
                ctx.source.printer().print(format!(
                    "代理 {} / 详细日志 {} / {} / 后台已跑 {} 轮",
                    onoff(app.proxy.load(Ordering::Relaxed)),
                    onoff(app.verbose.load(Ordering::Relaxed)),
                    if app.unlocked.load(Ordering::Relaxed) {
                        "已解锁"
                    } else {
                        "已上锁"
                    },
                    app.ticks.load(Ordering::Relaxed),
                ));
                1
            },
        ))
        // 两条指令下各有一个 `on` —— 说明挂在节点上，所以各说各的。
        .command(
            literal("proxy")
                .describe("上游代理开关")
                .then(switch("on", "启用上游代理", true, |app| &app.proxy))
                .then(switch(
                    "off",
                    "关掉上游代理，改为直连",
                    false,
                    |app| &app.proxy,
                )),
        )
        .command(
            literal("log")
                .describe("日志开关")
                .then(switch("on", "打开详细日志", true, |app| &app.verbose))
                .then(switch("off", "只留要紧的日志", false, |app| {
                    &app.verbose
                }))
                // 没写说明的参数：右侧退回 brigadier 自带的例子。
                .then(literal("level").describe("设定级别").then(
                    argument("level", integer()).executes(|ctx: &CommandContext<Src>| {
                        let level = get_integer(ctx, "level").unwrap_or(0);
                        ctx.source.printer().print(format!("级别设为 {level}"));
                        1
                    }),
                )),
        )
        .command(literal("unlock").describe("解锁危险指令").executes(
            |ctx: &CommandContext<Src>| {
                ctx.source.state().unlocked.store(true, Ordering::Relaxed);
                ctx.source.printer().print("已解锁，danger 现在可用了");
                1
            },
        ))
        .command(
            literal("lock")
                .describe("锁回去")
                .executes(|ctx: &CommandContext<Src>| {
                    ctx.source.state().unlocked.store(false, Ordering::Relaxed);
                    ctx.source.printer().print("已上锁");
                    1
                }),
        )
        // requires 判不过时，这条指令连菜单里都不会出现。判定用的是真实
        // 状态，所以「看得见」与「跑得动」始终是同一回事。
        .command(
            literal("danger")
                .describe("解锁之后才看得见的指令")
                .requires(|s: &Src| s.state().unlocked.load(Ordering::Relaxed))
                .executes(|ctx: &CommandContext<Src>| {
                    ctx.source.printer().print("砰");
                    1
                }),
        )
        .command(
            literal("slow")
                .describe("跑三秒，期间照样能输入下一条")
                .executes(|ctx: &CommandContext<Src>| {
                    ctx.source
                        .printer()
                        .print("开始…（试着现在就打下一条指令）");
                    std::thread::sleep(Duration::from_secs(3));
                    ctx.source.printer().print("跑完了");
                    1
                }),
        )
        .command(
            literal("boom")
                .describe("故意 panic —— 控制台会兜住它")
                .executes(|_: &CommandContext<Src>| -> i32 { panic!("演示用的 panic") }),
        )
        // 帮助是个生成器：默认分层实现，名字、说明、排版、路径不认识时说
        // 什么，都能换。不写这一行就没有 help。
        .command(
            smaragdine::help("help")
                .describe("显示指令帮助；help <指令> 往下看一层")
                .not_found("没有这条指令"),
        )
        .command(
            literal("stop")
                .describe("退出")
                .executes(|ctx: &CommandContext<Src>| {
                    ctx.source.request_exit(Bye::Stop);
                    1
                }),
        )
        .command(
            literal("restart")
                .describe("退出，并让主程序重来一遍")
                .executes(|ctx: &CommandContext<Src>| {
                    ctx.source.request_exit(Bye::Restart);
                    1
                }),
        )
        .build(Arc::clone(&state));

    // 后台往屏幕上写字：克隆一份接受器带走就行。它落在提示行上方，不会把
    // 你正在编辑的那一行搅乱 —— 真实程序里接的通常是日志系统。
    let printer = console.printer();
    let background = Arc::clone(&state);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let app = &background;
            let round = app.ticks.fetch_add(1, Ordering::Relaxed) + 1;
            if app.verbose.load(Ordering::Relaxed) {
                printer.print(format!(
                    "[后台] 第 {round} 轮，代理 {}",
                    onoff(app.proxy.load(Ordering::Relaxed))
                ));
            }
        }
    });

    // 控制台占住主线程，直到退出。
    let printer = console.printer();
    printer.print("输入 help 之外的任何东西都会被高亮解析；Tab 补全，Ctrl-C 退出");

    match console.run() {
        Exit::Quit(Bye::Stop) => printer.print("退出"),
        Exit::Quit(Bye::Restart) => printer.print("这里可以 exec 自己，演示就到此为止"),
        Exit::Interrupted => printer.print("收到 Ctrl-C / Ctrl-D"),
        // 没有终端（管道、后台服务、容器）—— 真实程序该继续跑，改由信号
        // 决定何时关停。
        Exit::NoTerminal => printer.print("没有可交互的终端，演示直接结束"),
        Exit::Failed(e) => eprintln!("控制台读取失败: {e}"),
    }
}

/// 一条把某个开关拨到固定位置的子指令。
fn switch(
    name: &str,
    about: &str,
    to: bool,
    pick: fn(&App) -> &AtomicBool,
) -> smaragdine::brigadier::builder::argument_builder::ArgumentBuilder<Src> {
    let name = name.to_owned();
    literal(&name)
        .describe(about)
        .executes(move |ctx: &CommandContext<Src>| {
            pick(ctx.source.state()).store(to, Ordering::Relaxed);
            ctx.source
                .printer()
                .print(format!("{name} —— 已{}", onoff(to)));
            1
        })
}

fn onoff(on: bool) -> &'static str {
    if on { "开" } else { "关" }
}
