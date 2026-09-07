<div align="center">

  <img src="./.github/assets/logo.svg" alt="Smaragdine" width="140">

  <h1>Smaragdine</h1>

  <p>
    <em>As above, so below.</em>
  </p>

  <p>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="License: Apache 2.0"></a>
  </p>

</div>

## About

Smaragdine 是一个在终端中实现 Minecraft 命令风格的交互式终端库，由魔改的 [Reedline](https://github.com/peter2500zz/reedline/tree/smaragdine) 与 [Azalea brigadier](https://github.com/peter2500zz/azalea/tree/smaragdine) 组成。

## Getting Started

### Installation

使用 cargo 添加依赖：

```bash
cargo add smaragdine --git https://github.com/peter2500zz/smaragdine.git
```

## Usage

以下是一个简单的示例，展示如何使用 Smaragdine 创建一个交互式终端，并注册一个 `ping` 命令：

```rust
use smaragdine::{Console, Source, brigadier::prelude::*};

/// 程序状态
#[derive(Default)]
struct App {}

fn main() {
    // 创建 smaragdine 控制台
    let console = Console::builder()
        // 以 Minecraft 风格注册命令
        .command(literal("ping").executes(ping))
        .build(App::default());

    // 运行控制台
    console.run();

    println!("Bye!");
}

fn ping(ctx: &CommandContext<Source<App>>) -> CommandResult {
    ctx.source.printer().print("pong!");
    Ok(1)
}
```

启用 `async` 特性将允许使用基于异步运行时的控制台以避免使用过多 OS 线程：

```rust
use std::sync::Arc;

use smaragdine::{AsyncConsole, Source, brigadier::prelude::*};

/// 程序状态
#[derive(Default)]
struct App {}

#[tokio::main]
async fn main() {
    // 获取 tokio 运行时
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .expect("build Tokio runtime");

    // 创建异步的 smaragdine 控制台
    let console = AsyncConsole::builder(runtime.handle().clone())
        // 以 Minecraft 风格注册命令
        .command(literal("ping").executes_async(ping))
        .build(App::default());

    // 运行控制台
    console.run();

    println!("Bye!");
}

/// 示例异步函数
async fn ping(ctx: Arc<CommandContext<Source<App>>>) -> CommandResult {
    ctx.source.printer().print("pong!");
    Ok(1)
}
```

启用 `macros` 特性将允许使用简易的宏语法构建命令：

```rust
use smaragdine::{Console, Source, brigadier::prelude::*, commands};

/// 程序状态
#[derive(Default)]
struct App {}

fn main() {
    // 创建 smaragdine 控制台
    let console = Console::builder()
        // 以 Minecraft 风格注册命令
        .commands(register_my_commands)
        .build(App::default());

    // 运行控制台
    console.run();

    println!("Bye!");
}

/// 使用宏注册命令
fn register_my_commands(d: &mut CommandDispatcher<Source<App>>) {
    commands!(d, {
        // ping
        literal("ping") => { run: ping; };
        // foo
        literal("foo") => {
            run: ping;
            // foo 123
            integer("bar") => { run: ping; };
            // foo true
            boolean("boom") => { run: ping; };
            // foo bar
            literal("bar") => { run: ping; };
        };
    })
}

fn ping(ctx: &CommandContext<Source<App>>) -> CommandResult {
    ctx.source.printer().print("pong!");
    Ok(1)
}
```

## Contributing

随意，如果你不介意 AI 代码的话。这本质是一个个人使用的库。

## License

Apache License 2.0
