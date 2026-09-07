//! 帮助：一个生成器，以及它底下的那个数据函数。
//!
//! 平铺列举整棵树在指令一多就没法看 —— `proxy on up` / `proxy on down` /
//! `proxy off good` / `proxy off bad` 会占掉四行，而这还只是一条指令。所以
//! 默认实现是**分层**的：一次只列一层，分支折成 `(on|off)`，想往下看就
//! `help proxy`。折叠由 brigadier 的 `get_smart_usage` 做，它顺带把节点交
//! 回来，于是每行都能配上节点自己的 `describe`，也已经按 `requires` 滤过。
//!
//! ```text
//! > help                          > help proxy            > help proxy on
//!   echo <message>  回显            proxy off (bad|good) 关   proxy on down  下行
//!   proxy (off|on)  上游代理        proxy on (down|up)   开   proxy on up    上行
//!   quit            退出
//! ```
//!
//! 生成器的每一环都能换：名字、说明、排版、路径不认识时说什么。想连指令
//! 一起自己写，就只用 [`usage`]。

use std::sync::Arc;

use azalea_brigadier::{
    builder::argument_builder::ArgumentBuilder,
    command_dispatcher::CommandDispatcher,
    context::CommandContext,
    prelude::*,
    suggestion::{Suggestions, SuggestionsBuilder},
    tree::CommandNode,
};
use parking_lot::RwLock;

use crate::{Source, Text};

type TreeNode<S, R> = Arc<RwLock<CommandNode<Source<S, R>>>>;

/// 帮助里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// 用法，形如 `proxy (off|on)` —— 分支已经折好，路径也补全了。
    pub usage: String,
    /// 这个节点的说明，也就是建树时写的 `describe()`。
    pub description: Option<String>,
}

/// 问某一层的用法。
///
/// `path` 是从根数下来的路径（`""` 就是顶层，`"proxy on"` 是那一支）。
/// 返回 `None` 表示这条路径不存在，或此刻 `requires` 判不过 —— 与「这一层
/// 底下没有东西了」（`Some(空表)`）是两回事。
///
/// ```
/// # use smaragdine::prelude::*;
/// # struct App;
/// # let console = Console::builder()
/// #     .command(literal("proxy").describe("上游代理")
/// #         .then(literal("on").describe("开").executes(
/// #             |_: &CommandContext<Source<App>>| -> CommandResult { Ok(1) }))
/// #         .then(literal("off").describe("关").executes(
/// #             |_: &CommandContext<Source<App>>| -> CommandResult { Ok(1) })))
/// #     .build(App);
/// let rows = smaragdine::usage(&console.dispatcher(), &console.source(), "proxy").unwrap();
///
/// assert_eq!(rows[0].usage, "proxy off");
/// assert_eq!(rows[0].description.as_deref(), Some("关"));
/// ```
pub fn usage<S, R>(
    tree: &CommandDispatcher<Source<S, R>>,
    source: &Source<S, R>,
    path: &str,
) -> Option<Vec<Usage>>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let node = resolve(&tree.root, source, path)?;

    let mut rows: Vec<Usage> = tree
        .get_smart_usage(&node.read(), source)
        .into_iter()
        .map(|(node, usage)| Usage {
            usage: if path.trim().is_empty() {
                usage
            } else {
                format!("{} {usage}", path.trim())
            },
            description: node.read().description.clone(),
        })
        .collect();

    // 排一下：`get_smart_usage` 遍历的是有序 map，但折出来的字符串未必跟着
    // 名字走，排过才保证两次调用给出同一个顺序。
    rows.sort_by(|a, b| a.usage.cmp(&b.usage));
    Some(rows)
}

/// 顺着路径走到那个节点。
fn resolve<S, R>(
    root: &TreeNode<S, R>,
    source: &Source<S, R>,
    path: &str,
) -> Option<TreeNode<S, R>> {
    let mut node = Arc::clone(root);

    for token in path.split_whitespace() {
        // 用法里参数写作 `<message>`，照着复制粘贴过来的路径也该认。
        let name = token.trim_start_matches('<').trim_end_matches('>');
        let child = node.read().child(name)?;
        if !child.read().can_use(source) {
            return None;
        }
        node = child;
    }

    Some(node)
}

/// 把 `ctx` 手里的那棵树套个壳，好问它用法。
///
/// 指令体拿得到的只有 `root_node()`，而折叠用法是 `CommandDispatcher` 上的
/// 方法。好在它的 `root` 是公开字段，换个壳就是同一棵树 —— 不必像别处那样
/// 重新建一棵，也不必把整个 dispatcher 塞进 source 里。
fn shell<S, R>(root: &TreeNode<S, R>) -> CommandDispatcher<Source<S, R>>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let mut tree = CommandDispatcher::new();
    tree.root = Arc::clone(root);
    tree
}

/// 怎么把一层用法打出来。
type Render<S, R> = Arc<dyn Fn(&Source<S, R>, &[Usage]) + Send + Sync>;

/// 帮助指令的生成器。
///
/// ```
/// # use smaragdine::prelude::*;
/// # struct App;
/// # let _ =
/// Console::<App>::builder()
///     .command(smaragdine::help("help").describe("显示指令帮助"))
/// # ;
/// ```
///
/// 每一环都能换 —— 换排版：
///
/// ```
/// # use smaragdine::prelude::*;
/// # struct App;
/// # let _ =
/// Console::<App>::builder().command(
///     smaragdine::help("?")
///         .describe("看看有什么指令")
///         .not_found("没有这条指令")
///         .render(|source, rows| {
///             for row in rows {
///                 source.printer().print(format!("· {}", row.usage));
///             }
///         }),
/// )
/// # ;
/// ```
pub struct Help<S, R = ()> {
    name: String,
    description: Option<String>,
    not_found: String,
    render: Render<S, R>,
}

/// 造一条帮助指令，名字由你定（`help`、`?`、`帮助` 都行）。
///
/// 造出来的是一条普通指令，交给 `command()` 注册 —— 库不会背着你往树里塞
/// 东西，不写这一行就没有 help。
pub fn help<S: 'static, R: 'static>(name: &str) -> Help<S, R> {
    Help {
        name: name.to_owned(),
        description: None,
        // 与 `Text` 的默认措辞一致；那边换了语言，这边通常也要跟着换。
        not_found: Text::default().unknown_command,
        render: Arc::new(print_rows),
    }
}

impl<S, R> Help<S, R> {
    /// 这条指令自己的说明，补全菜单里显示的就是它。
    pub fn describe(mut self, description: &str) -> Self {
        self.description = Some(description.to_owned());
        self
    }

    /// 路径不认识时说什么。
    pub fn not_found(mut self, text: &str) -> Self {
        self.not_found = text.to_owned();
        self
    }

    /// 换掉排版。默认是「用法 + 两空格 + 说明」，对齐到最长的那条用法。
    pub fn render(
        mut self,
        render: impl Fn(&Source<S, R>, &[Usage]) + Send + Sync + 'static,
    ) -> Self {
        self.render = Arc::new(render);
        self
    }
}

/// 默认排版。
fn print_rows<S, R>(source: &Source<S, R>, rows: &[Usage]) {
    let width = rows
        .iter()
        .map(|row| row.usage.chars().count())
        .max()
        .unwrap_or(0);

    for row in rows {
        match &row.description {
            Some(description) => source
                .printer()
                .print(format!("{:width$}  {description}", row.usage)),
            None => source.printer().print(&row.usage),
        }
    }
}

impl<S, R> From<Help<S, R>> for ArgumentBuilder<Source<S, R>, i32>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    fn from(help: Help<S, R>) -> Self {
        let Help {
            name,
            description,
            not_found,
            render,
        } = help;

        let top = {
            let (render, not_found) = (Arc::clone(&render), not_found.clone());
            move |ctx: &CommandContext<Source<S, R>>| -> CommandResult {
                show(ctx, "", &render, &not_found);
                Ok(1)
            }
        };
        let deeper = {
            // 最后一处用到，直接搬走。
            move |ctx: &CommandContext<Source<S, R>>| -> CommandResult {
                let path = get_string(ctx, "command").unwrap_or_default();
                show(ctx, &path, &render, &not_found);
                Ok(1)
            }
        };

        let mut node = literal(&name);
        if let Some(description) = description {
            node = node.describe(&description);
        }

        node.executes(top).then(
            // greedy：路径可以有好几段（`help proxy on`）。
            argument("command", greedy_string())
                .suggests(suggest_path::<S, R>)
                .executes(deeper),
        )
    }
}

fn show<S, R>(
    ctx: &CommandContext<Source<S, R>>,
    path: &str,
    render: &Render<S, R>,
    not_found: &str,
) where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let tree = shell(ctx.root_node());
    match usage(&tree, &ctx.source, path) {
        Some(rows) => render(&ctx.source, &rows),
        None => ctx.source.printer().print(not_found),
    }
}

/// 路径参数的候选：下一层的子指令。
///
/// 不给这个的话，`help pro` 一点提示都没有 —— 而树越深越需要提示，正是
/// 帮助本身要解决的问题。
fn suggest_path<S, R>(ctx: CommandContext<Source<S, R>>, builder: SuggestionsBuilder) -> Suggestions
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let typed = builder.remaining().to_owned();
    // 已经打完的那几段，与正在打的最后一段。
    let (walked, prefix) = match typed.rfind(char::is_whitespace) {
        Some(space) => (typed[..space].to_owned(), typed[space + 1..].to_owned()),
        None => (String::new(), typed.clone()),
    };

    let Some(node) = resolve(ctx.root_node(), &ctx.source, &walked) else {
        return builder.build();
    };

    // 候选替换的是最后一段，不是整条路径。
    let mut builder = builder.create_offset(builder.start() + (typed.len() - prefix.len()));
    // 只列字面量：钻进一个参数节点没有意义。
    for (name, child) in &node.read().literals {
        let child = child.read();
        if !child.can_use(&ctx.source) || !name.starts_with(&prefix) {
            continue;
        }
        builder = match &child.description {
            Some(description) => builder.suggest_with_tooltip(name, description.clone()),
            None => builder.suggest(name),
        };
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Console,
        testing::{Nothing, dispatcher, source},
    };

    fn rows(path: &str) -> Option<Vec<Usage>> {
        usage(&dispatcher(), &source(), path)
    }

    fn lines(path: &str) -> Vec<String> {
        rows(path)
            .expect("这条路径该存在")
            .into_iter()
            .map(|row| format!("{}|{}", row.usage, row.description.unwrap_or_default()))
            .collect()
    }

    /// 顶层：一条指令一行，分支折起来，各配各的说明。
    #[test]
    fn the_top_level_lists_one_line_per_command() {
        assert_eq!(
            lines(""),
            vec![
                "echo <message>|把参数原样输出",
                "log (level|on)|日志开关",
                "open|只有解锁时才可用",
                "proxy (off|on)|查看或修改上游代理设置",
                "quit|",
            ]
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_only_commands_are_listed() {
        let mut dispatcher = CommandDispatcher::new();
        dispatcher.register(literal("later").describe("异步执行").executes_async(
            |_: Arc<CommandContext<Source<Nothing>>>| async { Ok::<_, BoxCommandError>(1) },
        ));

        assert_eq!(
            usage(&dispatcher, &source(), ""),
            Some(vec![Usage {
                usage: "later".to_owned(),
                description: Some("异步执行".to_owned()),
            }])
        );
    }

    /// 往下钻一层，路径会补在前面。
    #[test]
    fn drilling_down_keeps_the_path() {
        assert_eq!(
            lines("proxy"),
            vec!["proxy off|关闭上游代理，改为直连", "proxy on|启用上游代理"]
        );
        assert_eq!(
            lines("log"),
            vec!["log level <level>|设定级别", "log on|打开详细日志"]
        );
    }

    /// 路径里带 `<>` 也认 —— 用法是那么印的，照着复制粘贴该能用。
    #[test]
    fn a_path_may_be_copied_out_of_a_usage_line() {
        assert_eq!(rows("log level <level>"), Some(Vec::new()));
        assert_eq!(rows("log level level"), Some(Vec::new()));
    }

    /// 走到头了是空表，不是「没有这条指令」—— 两者不能混。
    #[test]
    fn a_leaf_has_nothing_below_it() {
        assert_eq!(rows("quit"), Some(Vec::new()));
        assert_eq!(rows("zzz"), None);
        assert_eq!(rows("proxy zzz"), None);
    }

    /// requires 判不过的，帮助里也不该有 —— 与菜单一个口径。
    #[test]
    fn what_you_cannot_use_is_not_listed() {
        let locked = Source::new(Nothing { unlocked: false });
        let listed: Vec<String> = usage(&dispatcher(), &locked, "")
            .unwrap()
            .into_iter()
            .map(|row| row.usage)
            .collect();

        assert!(
            !listed.iter().any(|line| line.starts_with("open")),
            "{listed:?}"
        );
        assert_eq!(rows("open"), Some(Vec::new()), "解锁时才看得到");
        assert_eq!(usage(&dispatcher(), &locked, "open"), None);
    }

    /// 生成器造出来的是一条普通指令，注册进去就能跑。
    #[test]
    fn the_generated_command_runs() {
        let printed = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let console = Console::builder()
            .command(
                literal("quit")
                    .describe("退出")
                    .executes(|_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) }),
            )
            .command(help("help").describe("显示指令帮助").render({
                let printed = Arc::clone(&printed);
                move |_source, rows| {
                    printed
                        .lock()
                        .unwrap()
                        .extend(rows.iter().map(|r| r.usage.clone()));
                }
            }))
            .build(Nothing { unlocked: true });

        console
            .dispatcher()
            .execute("help", console.source())
            .expect("help 该能跑");
        // help 自己带一个可选参数，折叠用法把它标成 `[<command>]`。
        assert_eq!(*printed.lock().unwrap(), vec!["help [<command>]", "quit"]);

        printed.lock().unwrap().clear();
        console
            .dispatcher()
            .execute("help quit", console.source())
            .expect("help quit 该能跑");
        assert!(printed.lock().unwrap().is_empty(), "quit 底下没有东西");
    }

    /// 路径不认识时说的那句话是可换的。
    #[test]
    fn the_not_found_wording_is_yours() {
        let console = Console::builder()
            .command(help("help").not_found("没有这条指令"))
            .build(Nothing { unlocked: true });

        let printer = console.printer();
        let (sender, receiver) = crossbeam::channel::bounded(4);
        printer.attach(sender);

        console
            .dispatcher()
            .execute("help zzz", console.source())
            .expect("help 该能跑");
        assert_eq!(receiver.try_recv().as_deref(), Ok("没有这条指令"));
    }

    /// 路径参数要有候选，否则树越深越两眼一抹黑。
    #[test]
    fn the_path_argument_suggests_the_next_level() {
        let console =
            Console::builder()
                .command(
                    literal("proxy")
                        .describe("上游代理")
                        .then(literal("on").describe("开").executes(
                            |_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) },
                        ))
                        .then(literal("off").describe("关").executes(
                            |_: &CommandContext<Source<Nothing>>| -> CommandResult { Ok(1) },
                        )),
                )
                .command(help("help"))
                .build(Nothing { unlocked: true });

        let offered = |line: &str| -> Vec<(String, Option<String>)> {
            crate::completer::suggest(&console.dispatcher(), &console.source(), line, line.len())
                .into_iter()
                .map(|s| (s.value, s.description))
                .collect()
        };

        assert_eq!(
            offered("help pro"),
            vec![("proxy".to_owned(), Some("上游代理".to_owned()))]
        );
        assert_eq!(
            offered("help proxy o"),
            vec![
                ("off".to_owned(), Some("关".to_owned())),
                ("on".to_owned(), Some("开".to_owned())),
            ]
        );
    }
}
