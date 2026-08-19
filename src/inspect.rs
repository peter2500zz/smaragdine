//! 从一行输入里读出「不是候选」的那些结论。
//!
//! 补全菜单只该装真正能插进行里的候选。参数该填什么、这一行为什么不成立
//! ——这些是说明，不是候选，混进菜单会得到一份半真半假的列表：有的条目按
//! 一下会改动输入，有的按了什么也不会发生。
//!
//! 它们各有各的位置：
//!
//! | 结论 | 画在哪 |
//! |------|-------|
//! | 该填什么参数 | 光标处的幽灵文本，形如 `<message>` |
//! | 这个参数是什么 | 输入行右侧 |
//! | 这一行为什么不成立 | 输入行右侧 |
//!
//! 右侧那块由 reedline 的右提示承载，行一长它会自动不画，不会和输入打架。

use azalea_brigadier::{
    builder::argument_builder::ArgumentBuilderType, command_dispatcher::CommandDispatcher,
    context::CommandContextBuilder, string_reader::StringReader, tree::CommandNode,
};

use crate::{Context, Source, Text, theme::Piece};

/// 右侧那一句最多列几个示例。
///
/// 示例来自 brigadier 的 `ArgumentType::examples()`，双精度那种能给六个，
/// 全列出来会把提示行挤满。
const EXAMPLES: usize = 3;

/// 对一行输入的解析结论。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Inspection {
    /// 光标处该填的参数占位，形如 `<message>`。
    pub(crate) placeholder: Option<String>,
    /// 右侧那一句：出错时是原因，否则是这个参数是什么。
    pub(crate) aside: Option<(String, Piece)>,
}

/// 看一遍这行输入。
///
/// 解析包在 `catch_unwind` 里：这段代码对着每一次击键、以任意半成品输入
/// 运行。真崩了就当什么都没看出来，总好过把控制台带走。
pub(crate) fn inspect<C: Context>(
    dispatcher: &CommandDispatcher<Source<C>>,
    source: &Source<C>,
    text: &Text,
    line: &str,
    cursor: usize,
) -> Inspection {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let parse = dispatcher.parse(StringReader::from(line), source.clone());
        let context = parse.context.clone();
        let has_leftover = parse.reader.can_read();

        let has_candidates =
            !crate::completer::suggest(dispatcher, source, line, cursor).is_empty();

        // 占位只在「此刻无从下手」时才给：
        //
        // * 有候选就不给。这个位置若收的是字面量（`on` / `off`）或有限取值
        //   （`true` / `false`），菜单已经把能打的都列出来了，再来一句
        //   `<enabled>` 是废话。
        // * 当前这个 token 已经打了字也不给 —— 你正在写的就是它，不用人
        //   提醒这里是什么。
        //
        // 剩下的就是真正该提示的场合：没有可枚举取值的自由参数，比如
        // `<message>`。
        let expected = (!has_candidates && token_is_empty(&context, cursor))
            .then(|| expected_at(&context, cursor))
            .flatten();
        let placeholder = expected.as_ref().map(|(name, _)| format!("<{name}>"));

        let failure = (!has_candidates && placeholder.is_none())
            .then(|| failure(&context, text, line, has_leftover))
            .flatten();

        // 出错是警告，说明只是备注 —— 右侧那一块两种都可能出现，得说清是
        // 哪一种，配色才好各归各的。
        let aside = failure.map(|text| (text, Piece::Failure)).or_else(|| {
            expected
                .and_then(|(name, about)| about.map(|about| format!("<{name}> {about}")))
                .map(|text| (text, Piece::Hint))
        });

        Inspection { placeholder, aside }
    }))
    .unwrap_or_default()
}

/// 这一行为什么不成立，一句话。
///
/// 位置标记、出错 token 那些细节不在这里给 —— 右侧只有一行的地方，而回车
/// 执行时 brigadier 会把完整错误报出来。
fn failure<C: Context>(
    context: &CommandContextBuilder<'_, Source<C>, i32>,
    text: &Text,
    line: &str,
    has_leftover: bool,
) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    // 这行已经能跑了 —— 只是恰好没有更多可补的东西，不是出错。
    if !has_leftover && is_runnable(context) {
        return None;
    }

    // 一个节点都没匹配上 → 连指令名都不认识；匹配上了却还有剩余 → 参数不对。
    Some(if context.range.is_empty() {
        text.unknown_command.clone()
    } else {
        text.incorrect_argument.clone()
    })
}

/// 光标处那个 token 还是空的吗。
///
/// 建议上下文的起点就是当前 token 的起点；光标正好在起点上，说明这个 token
/// 一个字都还没打。
fn token_is_empty<C: Context>(
    context: &CommandContextBuilder<'_, Source<C>, i32>,
    cursor: usize,
) -> bool {
    if context.range.start() > cursor {
        return false;
    }
    context.find_suggestion_context(cursor).start_pos == cursor
}

/// 光标处该填的参数：(参数名, 它是什么)。
///
/// 「它是什么」优先取节点上写的说明；没写就退回 brigadier 自带的
/// `examples()`，那至少能让人看出该往里填什么形状的东西。
///
/// 用光标处的建议上下文取 parent，而不是 `context.nodes.last()` —— 参数已经
/// 打了一半时，最后一个匹配节点就是那个参数本身，它底下再没有参数了，
/// 于是问它会得到「什么都不缺」。
fn expected_at<C: Context>(
    context: &CommandContextBuilder<'_, Source<C>, i32>,
    cursor: usize,
) -> Option<(String, Option<String>)> {
    // 范围起点在光标之后时 find_suggestion_context 会 panic，先挡掉。
    if context.range.start() > cursor {
        return None;
    }
    let at_cursor = context.find_suggestion_context(cursor);
    let parent = at_cursor.parent.read();

    // 同一节点下多个参数时取字典序第一个，保证重绘之间稳定。`children` 正是
    // 按名字排好的（brigadier 那边的原话：children need to be ordered when
    // getting command suggestions），遍历它就行 —— `arguments` 是 HashMap，
    // 迭代序不定，得先收集再排序。
    let node = parent
        .children
        .values()
        .find(|child| matches!(child.read().value, ArgumentBuilderType::Argument(_)))?
        .read();

    Some((
        node.name().to_owned(),
        node.description.clone().or_else(|| examples_of(&node)),
    ))
}

/// 这个参数收什么形状的东西，用几个例子说明。
fn examples_of<C: Context>(node: &CommandNode<Source<C>>) -> Option<String> {
    let ArgumentBuilderType::Argument(argument) = &node.value else {
        return None;
    };

    let examples = argument.examples();
    (!examples.is_empty()).then(|| {
        examples
            .iter()
            .take(EXAMPLES)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    })
}

/// 这一行是否已经挂上了可执行的指令。
///
/// 重定向与子指令会把上下文串成一条链，只看最外层会漏掉后半截。
fn is_runnable<C: Context>(context: &CommandContextBuilder<'_, Source<C>, i32>) -> bool {
    context.command.is_some() || context.child.as_deref().is_some_and(is_runnable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dispatcher, source};

    fn look(line: &str) -> Inspection {
        let (dispatcher, source) = (dispatcher(), source());
        inspect(&dispatcher, &source, &Text::default(), line, line.len())
    }

    /// 该填参数时给出占位，右侧说明这个参数是什么 —— 说明取自节点。
    #[test]
    fn an_argument_position_offers_a_placeholder() {
        let seen = look("echo ");
        assert_eq!(seen.placeholder.as_deref(), Some("<message>"));
        assert_eq!(
            seen.aside,
            Some(("<message> 要输出的内容".to_owned(), Piece::Hint))
        );
    }

    /// 参数没写说明时退回 brigadier 的例子，至少看得出该填什么形状。
    #[test]
    fn an_undescribed_argument_falls_back_to_examples() {
        let seen = look("log level ");
        assert_eq!(seen.placeholder.as_deref(), Some("<level>"));
        assert_eq!(
            seen.aside,
            Some(("<level> 0 | 123 | -123".to_owned(), Piece::Hint))
        );
    }

    /// 参数打了字就不再提示 —— 你正在写的就是它。
    #[test]
    fn a_typed_token_needs_no_placeholder() {
        for line in ["echo hel", "echo 你好", "echo hi 还有更多"] {
            let seen = look(line);
            assert_eq!(seen.placeholder, None, "{line:?}");
            assert_eq!(seen.aside, None, "{line:?}");
        }
    }

    /// 有候选时也不提示：这个位置若收的是字面量或有限取值，菜单已经把能打的
    /// 列全了，再来一句 `<enabled>` 只是废话。
    #[test]
    fn candidates_crowd_out_the_placeholder() {
        for line in ["", "e", "pro", "proxy ", "log "] {
            assert_eq!(look(line).placeholder, None, "{line:?}");
        }
    }

    /// 指令名不认识 → 报未知指令，且不给占位。
    #[test]
    fn an_unknown_command_is_reported_without_a_placeholder() {
        let seen = look("zzz");
        assert_eq!(
            seen.aside,
            Some((Text::default().unknown_command, Piece::Failure))
        );
        assert_eq!(seen.placeholder, None);
    }

    /// 文案是可换的 —— 库只在这几处开口。
    #[test]
    fn the_wording_comes_from_the_text_table() {
        let (dispatcher, source) = (dispatcher(), source());
        let text = Text {
            unknown_command: "不认识的指令".to_owned(),
            ..Text::default()
        };

        let seen = inspect(&dispatcher, &source, &text, "zzz", 3);
        assert_eq!(
            seen.aside,
            Some(("不认识的指令".to_owned(), Piece::Failure))
        );
    }

    /// 空行什么都不说。
    #[test]
    fn an_empty_line_says_nothing() {
        assert_eq!(look(""), Inspection::default());
        assert_eq!(look("   "), Inspection::default());
    }

    fn is_failure(seen: &Inspection) -> bool {
        matches!(seen.aside, Some((_, Piece::Failure)))
    }

    /// 打得通的整行不该被扣上出错的帽子。
    #[test]
    fn a_runnable_line_is_not_an_error() {
        assert_eq!(look("quit").aside, None, "quit 没有参数，右侧该空着");
        assert!(!is_failure(&look("echo hi")));
        assert_eq!(look("echo hi").aside, None, "参数已经打了字，右侧也该空着");
    }

    /// 指令名打到一半只是还没打完，不是错。
    #[test]
    fn a_half_typed_command_is_not_an_error() {
        for line in ["e", "ec", "echo", "pro", "qui"] {
            assert!(!is_failure(&look(line)), "{line:?} 不该报错");
        }
    }

    /// 多余的 token 是参数错误。
    #[test]
    fn trailing_junk_is_an_argument_error() {
        assert_eq!(
            look("quit x").aside,
            Some((Text::default().incorrect_argument, Piece::Failure))
        );
    }

    /// 每次击键都会跑到它，任何输入都不能崩。
    #[test]
    fn survives_anything() {
        for line in [
            "你好",
            "echo 你好",
            "🎮",
            "café",
            "\\",
            "echo \\",
            "?",
            "  x  ",
        ] {
            let _ = look(line);
        }
    }
}
