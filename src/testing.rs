//! 测试共用的一棵小指令树。
//!
//! 树的形状是照着真实用法挑的，几个测试都指着它：
//!
//! * `echo <message>` —— 带参数，参数也写了说明
//! * `quit` —— 没写说明，用来验证「不该凭空造一句」
//! * `proxy on|off` 与 `log on|level <n>` —— 两个 `on` 共存，正是把说明挂在
//!   节点上才成立的事
//! * `open` 只在 `unlocked` 时可用 —— 验证 `requires` 与真实状态

use std::sync::Arc;

use azalea_brigadier::{command_dispatcher::CommandDispatcher, prelude::*};

use crate::Source;

/// 一份最小的状态。
pub(crate) struct Nothing {
    pub(crate) unlocked: bool,
}

pub(crate) fn source() -> Source<Nothing> {
    Source::new(Nothing { unlocked: true })
}

type Tree = CommandDispatcher<Source<Nothing>>;

pub(crate) fn dispatcher() -> Arc<Tree> {
    let mut tree: Tree = CommandDispatcher::new();

    tree.register(
        literal("echo").describe("把参数原样输出").then(
            greedy_string("message")
                .describe("要输出的内容")
                .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
        ),
    );

    // 刻意不写说明。
    tree.register(
        literal("quit").executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
    );

    tree.register(
        literal("proxy")
            .describe("查看或修改上游代理设置")
            .then(
                literal("on")
                    .describe("启用上游代理")
                    .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
            )
            .then(
                literal("off")
                    .describe("关闭上游代理，改为直连")
                    .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
            ),
    );

    tree.register(
        literal("log")
            .describe("日志开关")
            .then(
                // 与 proxy on 同名，各说各的。
                literal("on")
                    .describe("打开详细日志")
                    .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
            )
            .then(
                literal("level").describe("设定级别").then(
                    // 没写说明的参数，说明退回 brigadier 的 examples()。
                    integer("level")
                        .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
                ),
            ),
    );

    tree.register(
        literal("open")
            .describe("只有解锁时才可用")
            .requires(|s: &Source<Nothing>| s.state().unlocked)
            .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
    );

    Arc::new(tree)
}
