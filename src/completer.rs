//! 把 brigadier 的补全结果接到 reedline 的补全菜单上。
//!
//! 菜单里**只放真正能插进行里的候选**。参数该填什么、这一行为什么不成立，
//! 那些是说明不是候选 —— 混进来会得到一份半真半假的列表：有的条目按一下会
//! 改动输入，有的按了什么也不会发生。它们各有各的位置，见 [`crate::inspect`]。
//!
//! 于是候选为空是常态（打完整条指令、或在参数位置时），菜单直接不画 ——
//! `HidingMenu` 本来就这么处理。
//!
//! 两边的偏移量都是字节 —— brigadier 的 `StringRange` 在我们那份修正过游标
//! 语义的分支里是字节偏移，reedline 的 `Span` 本来就是。因此无需索引换算，
//! 中文输入下也不会错位。

use std::sync::Arc;

use azalea_brigadier::{
    command_dispatcher::CommandDispatcher, context::suggestion_context::SuggestionContext,
    string_reader::StringReader, tree::CommandNode,
};
use reedline::{Completer, CompletionResult, Span, Suggestion};

use crate::Source;

pub(crate) struct BrigadierCompleter<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    dispatcher: Arc<CommandDispatcher<Source<S, R>>>,
    source: Source<S, R>,
}

impl<S, R> BrigadierCompleter<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    pub(crate) fn new(
        dispatcher: Arc<CommandDispatcher<Source<S, R>>>,
        source: Source<S, R>,
    ) -> Self {
        Self { dispatcher, source }
    }
}

impl<S, R> Completer for BrigadierCompleter<S, R>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        CompletionResult::fresh(suggest(&self.dispatcher, &self.source, line, pos))
    }
}

/// 求一次补全。
///
/// 解析在 `catch_unwind` 里跑：这段代码对着每一次击键、以任意半成品输入
/// 运行，是整个控制台最容易被意外输入打到的地方。解析器崩了顶多这一次没有
/// 补全，不该把控制台一起带走。
pub(crate) fn suggest<S, R>(
    dispatcher: &CommandDispatcher<Source<S, R>>,
    source: &Source<S, R>,
    line: &str,
    pos: usize,
) -> Vec<Suggestion>
where
    S: Send + Sync + 'static,
    R: Send + 'static,
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let parse = dispatcher.parse(StringReader::from(line), source.clone());
        // get_completion_suggestions 会把 parse 吃掉，所以先把要用的留下来。
        let context = parse.context.clone();

        let mut suggestions: Vec<Suggestion> =
            CommandDispatcher::get_completion_suggestions_with_cursor(parse, pos)
                .list()
                .iter()
                .map(|suggestion| {
                    Suggestion {
                        // 说明随候选一起来 —— 节点上的 `describe()` 就落在这里。
                        description: suggestion.tooltip.clone(),
                        span: Span::new(suggestion.range.start(), suggestion.range.end()),
                        value: suggestion.text(),
                        // 不补空格：采用候选只该做区间替换、不追加任何东西。
                        // 终端分隔空格由 dispatcher 自己处理；这里追加空格会
                        // 改变可选子节点的解析位置，而且没有必要。
                        // 默认就是 false，写出来是因为这件事要紧。
                        append_whitespace: false,
                        ..Default::default()
                    }
                })
                .collect();

        // 范围起点在光标之后时 find_suggestion_context 会 panic，所以先挡掉。
        if context.range.start() <= pos {
            let at_cursor = context.find_suggestion_context(pos);
            {
                let parent = at_cursor.parent.read();
                suggestions.retain(|suggestion| usable(&parent, source, &suggestion.value));
            }
            suggestions.extend(exact_literal_match(&at_cursor, source, line, pos));
        }

        suggestions
    }));

    result.unwrap_or_default()
}

/// 这条候选此刻用得了吗。
///
/// 这是对 brigadier 的一处有意偏离：它的补全列举压根不看 `requires`
/// （`get_completion_suggestions_with_cursor` 直接遍历 `parent.children`），
/// 因为 Mojang 那边是给每个玩家下发过滤好的树，客户端根本看不到用不了的
/// 指令。而控制台是一棵共享的树，`requires` 就是唯一的闸门 —— 不在这里滤，
/// 同一条指令会同时有三种说法：菜单里有它、输进去标红、回车说不认识。
fn usable<S, R>(parent: &CommandNode<Source<S, R>>, source: &Source<S, R>, value: &str) -> bool {
    // 字面量对得上，就问它自己。
    if let Some(node) = parent.literals.get(value) {
        return node.read().can_use(source);
    }

    // 其余的值来自参数节点（bool 的 true/false、自定义候选提供者……）。
    // brigadier 没说是哪一个给的，所以只要还有一个用得了的参数子节点就放行
    // —— 一个都没有时，这些值无论如何也落不到实处。
    parent.arguments.is_empty()
        || parent
            .arguments
            .values()
            .any(|node| node.read().can_use(source))
}

/// 把指令名恰好打全时的那一条候选补回来。
///
/// brigadier 会刻意丢弃与已输入完全相同的候选 —— `SuggestionsBuilder::suggest`
/// 里的 `text == remaining` 短路，即「补了等于没补」就不给。
/// 游戏里这不成问题：指令栏上方还有一条独立的错误提示兜底。而这里补全菜单
/// 是唯一的反馈通道，指令名打全反而什么都不显示，看着倒像是打错了。
///
/// 所以这里有意偏离一点：合法指令名照常列出自身，说明照旧从节点上取。
fn exact_literal_match<S, R>(
    at_cursor: &SuggestionContext<Source<S, R>, i32>,
    source: &Source<S, R>,
    line: &str,
    pos: usize,
) -> Option<Suggestion> {
    let start = at_cursor.start_pos.min(pos);
    if pos > line.len() || !line.is_char_boundary(start) || !line.is_char_boundary(pos) {
        return None;
    }

    let typed = &line[start..pos];
    if typed.is_empty() {
        return None;
    }

    // 只认完全一致的那一条：前缀匹配 brigadier 自己已经给过了。
    let node = at_cursor.parent.read().literals.get(typed).cloned()?;
    let node = node.read();
    if !node.can_use(source) {
        return None;
    }
    let description = node.description.clone();

    Some(Suggestion {
        description,
        span: Span::new(start, pos),
        value: typed.to_owned(),
        // 与上面同理：终端分隔空格由 dispatcher 处理，候选本身不追加。
        append_whitespace: false,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Nothing, dispatcher, source};
    use azalea_brigadier::prelude::*;

    fn suggestions(line: &str) -> Vec<Suggestion> {
        let (dispatcher, source) = (dispatcher(), source());
        suggest(&dispatcher, &source, line, line.len())
    }

    fn values(line: &str) -> Vec<String> {
        suggestions(line).iter().map(|s| s.value.clone()).collect()
    }

    /// 候选是按字典序排的，所以按值找而不是按下标取。
    fn description_of(line: &str, value: &str) -> Option<String> {
        suggestions(line)
            .into_iter()
            .find(|s| s.value == value)
            .unwrap_or_else(|| panic!("{line:?} 该给出候选 {value:?}"))
            .description
    }

    #[test]
    fn suggests_command_names_from_a_prefix() {
        assert_eq!(values("ec"), vec!["echo"]);
    }

    #[test]
    fn suggests_every_command_on_an_empty_line() {
        assert!(values("").contains(&"echo".to_owned()), "{:?}", values(""));
    }

    /// 菜单要显示简介，缺了就只剩一列光秃秃的指令名。说明来自节点上的
    /// `describe()`，随 brigadier 的 tooltip 一起流过来。
    #[test]
    fn suggestions_carry_their_description() {
        assert_eq!(
            description_of("ec", "echo").as_deref(),
            Some("把参数原样输出")
        );
    }

    /// 没写说明的节点就没有说明 —— 不该凭空造一句。
    #[test]
    fn an_undescribed_command_has_no_description() {
        assert_eq!(description_of("qui", "quit"), None);
    }

    /// span 用来决定替换掉输入里的哪一段，错了就会把已输入的字符吃掉或重复。
    #[test]
    fn span_covers_exactly_the_typed_prefix() {
        assert_eq!(suggestions("ec")[0].span, Span::new(0, 2));
        assert_eq!(suggestions("ec")[0].value, "echo");
    }

    /// 指令名不认识时没有任何可插入的候选。
    #[test]
    fn an_unknown_command_offers_nothing() {
        assert!(suggestions("zzz").is_empty());
    }

    /// 补全不应替候选追加空格。终端分隔空格本身是合法的，
    /// 但候选只负责替换当前跨度；追加空格会改变下一次解析的位置。
    #[test]
    fn completions_never_append_whitespace() {
        for line in ["ec", "e", "qu", "qui"] {
            for suggestion in suggestions(line) {
                assert!(
                    !suggestion.append_whitespace,
                    "{line:?} 的候选 {:?} 会补出尾随空格",
                    suggestion.value
                );
            }
        }

        // 直接核对带终端分隔空格的整行确实能执行。
        let (dispatcher, source) = (dispatcher(), source());
        assert!(dispatcher.execute("quit", source.clone()).is_ok());
        assert!(dispatcher.execute("quit ", source.clone()).is_ok());
        assert!(dispatcher.execute("quit  ", source).is_ok());
    }

    #[test]
    fn optional_arguments_ignore_terminal_spaces() {
        let mut tree: CommandDispatcher<Source<Nothing>> = CommandDispatcher::new();
        tree.register(
            literal("kick").then(
                argument("player", word())
                    .executes(|_| 1)
                    .then(argument("reason", greedy_string()).executes(|_| 2)),
            ),
        );
        let source = source();

        for line in ["kick Bob", "kick Bob ", "kick Bob  ", "kick Bob   "] {
            assert_eq!(tree.execute(line, source.clone()).unwrap(), 1, "{line:?}");
        }
        assert_eq!(tree.execute("kick Bob reason", source).unwrap(), 2);
    }

    #[test]
    fn optional_literal_is_suggested_after_terminal_space_run() {
        let mut tree: CommandDispatcher<Source<Nothing>> = CommandDispatcher::new();
        tree.register(
            literal("kick")
                .then(argument("player", word()).then(literal("reason").executes(|_| 2))),
        );
        let source = source();

        for line in ["kick Bob ", "kick Bob  ", "kick Bob   "] {
            let offered = suggest(&tree, &source, line, line.len());
            assert_eq!(offered.len(), 1, "{line:?}");
            assert_eq!(offered[0].value, "reason", "{line:?}");
            assert_eq!(offered[0].span.end, line.len(), "{line:?}");
            assert_eq!(offered[0].span.start, "kick Bob".len() + 1, "{line:?}");
        }
    }

    /// 指令名打全之后，自身与同前缀的更长指令都要列出来。
    ///
    /// brigadier 只丢弃「与已输入完全相同」的那一条（`SuggestionsBuilder::suggest`
    /// 里的 `text == remaining` 短路），更长的同前缀指令本来就不受影响。自身
    /// 那一条由 exact_literal_match 补回。
    #[test]
    fn longer_commands_sharing_a_prefix_still_appear() {
        let mut tree: CommandDispatcher<Source<Nothing>> = CommandDispatcher::new();
        for name in ["echo", "echobig", "exit"] {
            tree.register(literal(name).executes(|_: &CommandContext<Source<Nothing>>| 1));
        }
        let (tree, source) = (Arc::new(tree), source());

        let values = |line: &str| -> Vec<String> {
            suggest(&tree, &source, line, line.len())
                .iter()
                .map(|s| s.value.clone())
                .collect()
        };

        assert_eq!(values("e"), vec!["echo", "echobig", "exit"]);
        assert_eq!(values("ec"), vec!["echo", "echobig"]);
        // brigadier 丢掉了与已输入相同的 echo，由 exact_literal_match 补回来；
        // echobig 则一直都在。
        assert_eq!(values("echo"), vec!["echobig", "echo"]);
        assert_eq!(values("echobig"), vec!["echobig"]);
    }

    /// 同名子指令各说各的 —— 这正是把说明挂在节点上换来的。
    #[test]
    fn subcommands_sharing_a_name_keep_their_own_description() {
        assert_eq!(
            description_of("proxy o", "on"),
            Some("启用上游代理".to_owned())
        );
        assert_eq!(
            description_of("log o", "on"),
            Some("打开详细日志".to_owned())
        );
    }

    /// 菜单里只能有真正插得进去的候选。
    #[test]
    fn the_menu_holds_only_insertable_candidates() {
        for line in [
            "",
            "e",
            "ec",
            "echo",
            "echo ",
            "echo hi",
            "echo 你好",
            "zzz",
            "你好",
            "🎮",
            "quit",
            "quit x",
            "pro",
            "proxy ",
            "log ",
        ] {
            for suggestion in suggestions(line) {
                assert!(!suggestion.value.is_empty(), "{line:?} 混进了纯展示条目");
                assert!(
                    suggestion.display_override.is_none(),
                    "{line:?} 混进了纯展示条目"
                );
            }
        }
    }

    /// 补全在每次击键时运行，非 ASCII 输入必须既不崩也不错位。
    #[test]
    fn survives_non_ascii_input() {
        for line in ["你好", "echo 你好", "🎮", "café", "echo 你好 世"] {
            let _ = suggestions(line);
        }
    }

    /// span 是字节偏移。若 brigadier 那边按字符计数，中文前缀就会算错。
    #[test]
    fn spans_are_byte_offsets() {
        let line = "echo 你好";
        assert_eq!(line.len(), 11);
        assert_eq!(line.chars().count(), 7);

        for suggestion in suggestions(line) {
            assert!(suggestion.span.end <= line.len());
            assert!(line.is_char_boundary(suggestion.span.start));
            assert!(line.is_char_boundary(suggestion.span.end));
        }
    }

    /// requires 判不过的指令不该出现在菜单里 —— 而它判定时拿到的是真实
    /// 状态，所以「看得见」与「跑得动」始终是同一回事。
    #[test]
    fn a_command_you_cannot_use_is_not_offered() {
        let mut tree: CommandDispatcher<Source<Nothing>> = CommandDispatcher::new();
        tree.register(
            literal("open")
                .requires(|s: &Source<Nothing>| s.state().unlocked)
                .executes(|_: &CommandContext<Source<Nothing>>| 1),
        );
        let tree = Arc::new(tree);

        let locked = Source::new(Nothing { unlocked: false });
        let unlocked = Source::new(Nothing { unlocked: true });

        assert!(suggest(&tree, &locked, "o", 1).is_empty(), "锁着时不该列出");
        assert_eq!(
            suggest(&tree, &unlocked, "o", 1)
                .iter()
                .map(|s| s.value.clone())
                .collect::<Vec<_>>(),
            vec!["open"]
        );
    }
}
