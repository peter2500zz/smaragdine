# smaragdine — 给 AI 看的方案备忘

一句话：把 mojang-auth-proxy 的交互式控制台（brigadier 指令树 + reedline 补全/高亮）
抽成一个库，易用但不损失拓展性。

参考实现：`~/mojang-auth-proxy/src/console.rs` 与 `src/console/*.rs`。

## 已确定

| 决策 | 结论 | 备注 |
|------|------|------|
| 交付形态 | 纯库 crate（`src/lib.rs`）+ `examples/demo.rs` | 删掉 `src/main.rs`；库里不含任何业务指令 |
| 用户上下文与退出意向 | `Context` trait + 关联类型 `Exit`，源类型是 `Source<C>` | 一个泛型参数；日后加带默认实现的方法不破坏兼容 |

用户侧长这样：

```rust
struct App { db: Db }
enum Bye { Stop(i32), Restart, Forced(i32) }

impl smaragdine::Context for App { type Exit = Bye; }
type Src = smaragdine::Source<App>;

literal("restart").executes(|ctx: &CommandContext<Src>| {
    ctx.source.request_exit(Bye::Restart);
    1
});

match console.run() {
    Exit::Quit(Bye::Restart) => exec_self(),
    Exit::NoTerminal         => wait_for_signal(),   // 没有 tty，交给信号
}
```

## 待定

- 指令说明（补全菜单右边那句）挂在哪：改 fork 让节点自带 `describe()`，还是库侧维护说明表
- 后台日志怎么打在提示行上方（`ExternalPrinter` 的接线方式，是否绑定 tracing）
- 库要不要自带 help / 退出类指令
- 配色、提示符、按键表的可定制程度
- 测试范围（单元测试照搬 + 是否要 pty 集成测试）

## 硬依赖：两个 fork

| fork | 为什么非它不可 |
|------|----------------|
| `peter2500zz/azalea` 分支 `fix/utf8-cursor` | 上游 `StringReader` 把字符数当字节偏移用，任何非 ASCII 输入都 panic |
| `peter2500zz/reedline` 分支 `feat/menu-accept` | 上游没有「采用候选但不关菜单」的事件，Tab 无法原地循环，只能一条条摞进行里 |

## 参考实现里各模块的职责（提取时对照）

| 模块 | 做的事 | 与业务的纠缠 |
|------|--------|--------------|
| `completer` | brigadier 候选 → reedline 菜单；只放真正插得进去的候选 | 说明表 |
| `inspect` | 从一行输入读出「不是候选」的结论：该填什么参数、为什么不成立 | 说明表 |
| `highlighter` | 按解析结果上色 + 光标处幽灵文本（预览选中候选 / `<参数名>`） | 无 |
| `menu` | `HidingMenu`：候选为空时活着但不画；`MenuCursor` / `MenuVisible` | 无 |
| `keys` | 三层按键分派：弹窗 → 编辑框 → 历史；Tab 原地循环；回车永远是提交 | 无 |
| `history` | 摘掉 reedline 的前缀过滤，↑↓ 退化成纯索引走位 | 无 |
| `interrupt` | 两段式 Ctrl-C（有内容只抹掉，空行连按两次才退出） | 无 |
| `prompt` | 提示符 + 右侧那一句（`Aside`，由高亮器填） | 无 |
| `commands` | 指令注册、`ConsoleSource`、`ExitAction`、说明表 | 全是业务 |
| `logging::ConsoleSink` | 日志改投 `ExternalPrinter`，控制台退出后回到 stdout | 在 logging 模块里，要一并提取 |

## 开发规范

- 关键能力决策、新功能/改动一律先用 AskUserQuestion 与人确认，一次一个问题
- 勤提交
- 注释用中文，讲「为什么」而不是「是什么」，与参考项目同一风格
- edition 2024，`unsafe_code = "forbid"`，模块布局用 `foo.rs + foo/`（不用 `mod.rs`）
