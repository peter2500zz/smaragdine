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
        .command(
            literal("ping").executes(|ctx: &CommandContext<Source<App>>| -> CommandResult {
                ctx.source.printer().print("pong!");
                Ok(1)
            }),
        )
        .build(App::default());

    // 运行控制台
    console.run();

    println!("Bye!");
}
```

## Contributing

随意，如果你不介意 AI 代码的话。这本质是一个个人使用的库。

## License

Apache License 2.0
