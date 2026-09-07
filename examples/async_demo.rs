//! Tokio-task-backed console demo.
//!
//! ```shell
//! cargo run --example async_demo --features async-demo
//! ```
//!
//! Run `slow`, then immediately submit `ping`: the timer yields to Tokio and no
//! command-specific operating-system thread is created.

use std::{sync::Arc, time::Duration};

use smaragdine::{Text, prelude::*, tokio};

struct App;

enum Bye {
    Stop,
}

type State = Arc<App>;
type Src = Source<State, Bye>;

async fn ping(ctx: Arc<CommandContext<Src>>) -> CommandResult {
    ctx.source.printer().print("pong");
    Ok(1)
}

async fn slow(ctx: Arc<CommandContext<Src>>) -> CommandResult {
    ctx.source
        .printer()
        .print("开始等待；现在可以继续输入 ping");
    tokio::time::sleep(Duration::from_secs(3)).await;
    ctx.source.printer().print("等待结束");
    Ok(1)
}

async fn stop(ctx: Arc<CommandContext<Src>>) -> CommandResult {
    ctx.source.request_exit(Bye::Stop);
    Ok(1)
}

fn main() {
    // Smaragdine owns only this Handle. The application owns the runtime and
    // decides its scheduler, drivers, worker count, and shutdown policy.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .expect("build Tokio runtime");

    let console = AsyncConsole::<State, Bye>::builder_with_reason(runtime.handle().clone())
        .text(Text {
            command_panicked: "异步指令 panic；控制台已兜住".to_owned(),
            ..Text::default()
        })
        .prompt("async> ")
        .command(literal("ping").describe("立即回应").executes_async(ping))
        .command(
            literal("slow")
                .describe("异步等待三秒")
                .executes_async(slow),
        )
        // 短小且不阻塞的旧式同步 action 也能迁移进来；它直接跑在 Tokio
        // worker 上，耗时工作仍应改成 executes_async。
        .command(literal("state").describe("读取应用状态").executes(
            |ctx: &CommandContext<Src>| -> CommandResult {
                let _ = ctx.source.state();
                ctx.source.printer().print("状态可用");
                Ok(1)
            },
        ))
        .command(
            smaragdine::help("help")
                .describe("显示帮助")
                .not_found("没有这条指令"),
        )
        .command(literal("stop").describe("退出").executes_async(stop))
        .build(Arc::new(App));

    let printer = console.printer();
    printer.print("输入 help；slow 使用 Tokio timer，Console::run 仍占据当前线程");

    match console.run() {
        Exit::Quit(Bye::Stop) => printer.print("退出"),
        Exit::Interrupted => printer.print("收到 Ctrl-C / Ctrl-D"),
        Exit::NoTerminal => printer.print("没有可交互的终端，演示直接结束"),
        Exit::Failed(error) => eprintln!("控制台读取失败: {error}"),
    }
}
