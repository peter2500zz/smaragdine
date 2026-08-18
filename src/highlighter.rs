//! 基于 brigadier 实时解析结果的语法高亮。
//!
//! 一行被切成三类片段交给着色闭包：字面量与空白、参数（带出现序号）、解析
//! 不掉的尾巴。默认配色见 [`crate::default_paint`]，与游戏内的指令栏一致。
//!
//! 注意片段的分类**既不表示合法性、也不表示类型**。指令名无论对错都是字面
//! 量 —— 打错时之所以显示为红，是因为整行都没被解析掉，落进了「尾巴」那
//! 一段。
//!
//! 光标处的幽灵文本也在这里画：菜单里选中了候选就预览它的剩余部分，没有
//! 候选而此处该填参数就显示 `<参数名>`。之所以由高亮器来画而不是 `Hinter`，
//! 是因为 `Hinter` 印在整行之后、且菜单一激活就根本不印，落不到光标处。

use std::sync::Arc;

use azalea_brigadier::{
    command_dispatcher::CommandDispatcher,
    context::{CommandContextBuilder, StringRange},
    string_reader::StringReader,
};
use nu_ansi_term::Style;
use reedline::{Highlighter, StyledText, Suggestion};

use crate::{
    Context, Source, Text, inspect,
    interrupt::LineShadow,
    menu::MenuCursor,
    prompt::Aside,
    theme::{Paint, Piece, Token},
};

pub(crate) struct BrigadierHighlighter<C: Context> {
    dispatcher: Arc<CommandDispatcher<Source<C>>>,
    source: Source<C>,
    text: Text,
    paint: Paint,
    /// 顺手记下当前行，供两段式 Ctrl-C 判断按下时行是否为空。
    ///
    /// 高亮器是 reedline 里唯一每次重绘都能拿到完整输入行的位置，而
    /// `read_line` 返回 Ctrl-C 时行已经被清掉了。
    shadow: LineShadow,
    /// 菜单里选中第几条，用来预览它的剩余部分。
    cursor: MenuCursor,
    /// 输入行右侧那一句，由这里填。
    aside: Aside,
}

impl<C: Context> BrigadierHighlighter<C> {
    pub(crate) fn new(
        dispatcher: Arc<CommandDispatcher<Source<C>>>,
        source: Source<C>,
        text: Text,
        paint: Paint,
        shadow: LineShadow,
        cursor: MenuCursor,
        aside: Aside,
    ) -> Self {
        Self {
            dispatcher,
            source,
            text,
            paint,
            shadow,
            cursor,
            aside,
        }
    }
}

impl<C: Context> Highlighter for BrigadierHighlighter<C> {
    fn highlight(&self, line: &str, cursor: usize) -> StyledText {
        self.shadow.record(line);

        // 一次解析，两处产出：光标处的占位，与右侧那一句。
        let seen = inspect::inspect(&self.dispatcher, &self.source, &self.text, line, cursor);
        self.aside.set(seen.aside.clone());

        let mut styled = styled_line(&self.dispatcher, &self.source, &self.paint, line);
        // 幽灵文本插在光标那个字节偏移上，于是被切进 after_cursor 的最前面，
        // 紧挨着光标印出（`render_around_insertion_point` 是按字节走段、在
        // 光标处切开的）。插在光标**之前**会让切分点错位，所以只能插这里。
        if let Some(ghost) = self.ghost(line, cursor, seen.placeholder) {
            styled = with_ghost(styled, cursor.min(line.len()), &ghost, &self.paint);
        }
        styled
    }
}

impl<C: Context> BrigadierHighlighter<C> {
    /// 光标处该显示什么灰字。
    ///
    /// 两种来源互斥：菜单里选中了候选就预览它的剩余部分（Tab 才真的插入），
    /// 没有候选而此处该填参数就显示 `<参数名>`。
    fn ghost(&self, line: &str, at: usize, placeholder: Option<String>) -> Option<String> {
        if self.menu_is_up(line) {
            // 候选现算，不问菜单要 —— 菜单要等重绘才更新，问它会慢一帧。
            let candidates = crate::completer::suggest(&self.dispatcher, &self.source, line, at);
            if let Some(index) = self.cursor.index(candidates.len())
                && let Some(remainder) = remainder_of(&candidates[index], line, at)
            {
                return Some(remainder);
            }
        }
        placeholder
    }

    /// 补全菜单此刻是不是真的开着。
    ///
    /// 幽灵预览的是「菜单里选中的那条」，所以前提是菜单确实在。拿「有没有
    /// 候选」当这个前提的替身是不等价的：空行的候选是**全部指令**，可空行上
    /// 菜单根本没开 —— 于是屏幕上会凭空多出一段预览，预览的还是字典序第
    /// 一条。
    ///
    /// 这里按 reedline 自己关菜单的条件来判，而不是问菜单要状态（`MenuVisible`
    /// 要等重绘才更新，会慢一帧）：
    ///
    /// * 缓冲区空了它就停用菜单（`engine.rs` 的编辑处理末尾）；
    /// * Esc 收起（由按键策略记下，见 [`MenuCursor`]）。
    ///
    /// 「有没有候选」那一条留在调用处 —— 没有候选自然也没得预览。
    fn menu_is_up(&self, line: &str) -> bool {
        !line.is_empty() && !self.cursor.is_dismissed()
    }
}

/// 选中候选相对于已输入内容的剩余部分。
///
/// 候选的 `span` 指的是它要替换掉的那一段。只有当已输入的正是它的前缀、
/// 且光标恰在跨度末尾时，剩余部分才是「接着打下去会得到的东西」——
/// 否则预览会骗人，宁可不显示。
fn remainder_of(selected: &Suggestion, line: &str, cursor: usize) -> Option<String> {
    let (start, end) = (selected.span.start, selected.span.end);
    if end != cursor || start > end || end > line.len() {
        return None;
    }
    if !line.is_char_boundary(start) || !line.is_char_boundary(end) {
        return None;
    }
    let typed = &line[start..end];
    let rest = selected.value.strip_prefix(typed)?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// 把 `ghost` 插进 `styled` 的 `at` 字节处。
fn with_ghost(styled: StyledText, at: usize, ghost: &str, paint: &Paint) -> StyledText {
    let style = paint(&Token::new(Piece::Ghost, 0, ghost));
    let mut out = StyledText::new();
    let mut seen = 0usize;
    let mut inserted = false;

    for (existing, text) in styled.buffer {
        if !inserted && seen == at {
            out.push((style, ghost.to_owned()));
            inserted = true;
        }
        seen += text.len();
        out.push((existing, text));
    }
    if !inserted {
        out.push((style, ghost.to_owned()));
    }
    out
}

/// 给一行输入上色。
///
/// 与补全同理，解析包在 `catch_unwind` 里：这段代码对着每一次击键、以任意
/// 半成品输入运行。真崩了就退回无着色的纯文本，总好过把控制台带走。
fn styled_line<C: Context>(
    dispatcher: &CommandDispatcher<Source<C>>,
    source: &Source<C>,
    paint: &Paint,
    line: &str,
) -> StyledText {
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let parse = dispatcher.parse(StringReader::from(line), source.clone());
        let arguments = argument_ranges(&parse.context);
        // 取 reader 的 cursor 与剩余长度来划出「没解析掉的那一截」。
        let tail = parse
            .reader
            .can_read()
            .then(|| (parse.reader.cursor(), parse.reader.remaining_length()));
        (arguments, tail)
    }));

    let Ok((arguments, tail)) = parsed else {
        return plain(line);
    };

    let mut styled = StyledText::new();
    let mut at = 0usize;

    // 参数带着出现序号交出去，于是「按顺序循环取色」这种方案写得出来。
    for (index, range) in arguments.iter().enumerate() {
        let (start, end) = (range.start(), range.end());
        if start < at || end > line.len() || start > end {
            continue;
        }
        if !line.is_char_boundary(start) || !line.is_char_boundary(end) {
            continue;
        }
        push(&mut styled, paint, Piece::Literal, 0, &line[at..start]);
        push(
            &mut styled,
            paint,
            Piece::Argument,
            index,
            &line[start..end],
        );
        at = end;
    }

    // 没解析掉的尾巴。
    if let Some((cursor, remaining)) = tail {
        let start = cursor.max(at).min(line.len());
        let end = (cursor + remaining).min(line.len());
        if start < end && line.is_char_boundary(start) && line.is_char_boundary(end) {
            push(&mut styled, paint, Piece::Literal, 0, &line[at..start]);
            push(&mut styled, paint, Piece::Unparsed, 0, &line[start..end]);
            at = end;
        }
    }

    push(&mut styled, paint, Piece::Literal, 0, &line[at..]);
    styled
}

fn push(styled: &mut StyledText, paint: &Paint, piece: Piece, index: usize, text: &str) {
    if !text.is_empty() {
        styled.push((paint(&Token::new(piece, index, text)), text.to_owned()));
    }
}

fn plain(line: &str) -> StyledText {
    let mut styled = StyledText::new();
    styled.push((Style::new(), line.to_owned()));
    styled
}

/// 各参数占据的区间，按起点排序。
///
/// 取最深一层子上下文的参数集合。Brigadier 原本靠有序 map 的插入序，而
/// azalea 用的是 `HashMap`，迭代序不定，所以这里按区间起点排 —— 效果等同
/// 且稳定（否则颜色会在重绘之间乱跳）。
fn argument_ranges<C: Context>(
    context: &CommandContextBuilder<'_, Source<C>, i32>,
) -> Vec<StringRange> {
    let mut deepest = context;
    while let Some(child) = &deepest.child {
        deepest = child;
    }

    let mut ranges: Vec<StringRange> = deepest.arguments.values().map(|a| a.range).collect();
    ranges.sort_by_key(StringRange::start);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        testing::{Nothing, dispatcher, source},
        theme::default_paint,
    };
    use nu_ansi_term::Color;

    fn paint() -> Paint {
        Arc::new(default_paint)
    }

    /// 取出 (文本, 颜色) 序列，忽略纯空白片段。
    fn spans(line: &str) -> Vec<(String, Option<Color>)> {
        styled_line(&dispatcher(), &source(), &paint(), line)
            .buffer
            .into_iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(style, text)| (text, style.foreground))
            .collect()
    }

    fn assert_roundtrips(line: &str) {
        let rebuilt: String = styled_line(&dispatcher(), &source(), &paint(), line)
            .buffer
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(rebuilt, line, "高亮改动了输入内容");
    }

    fn colour(piece: Piece, index: usize) -> Option<Color> {
        default_paint(&Token::new(piece, index, "x")).foreground
    }

    /// 指令名是字面量 —— 分类不表示合法性。
    #[test]
    fn a_command_name_is_a_literal() {
        // 参数前的空白一并算进字面量段，所以这里是 "echo " 而非 "echo"。
        let spans = spans("echo hello");
        assert_eq!(spans[0].0, "echo ");
        assert_eq!(spans[0].1, colour(Piece::Literal, 0));
    }

    /// 参数带着出现序号交给闭包，默认配色于是循环取色。
    #[test]
    fn arguments_carry_their_position() {
        let spans = spans("echo hello");
        assert_eq!(spans[1].0, "hello");
        assert_eq!(spans[1].1, colour(Piece::Argument, 0));
    }

    /// 打错指令名时整行都没被解析，于是整行落进「尾巴」。
    #[test]
    fn an_unparsed_line_is_all_tail() {
        let spans = spans("ecoh hello");
        assert!(
            spans
                .iter()
                .all(|(_, colour_of)| *colour_of == colour(Piece::Unparsed, 0)),
            "{spans:?}"
        );
    }

    /// 还没输完的前缀同样没解析掉 —— 输到 echo 才转为字面量。
    #[test]
    fn a_partial_command_is_a_tail_until_it_parses() {
        assert_eq!(spans("ec")[0].1, colour(Piece::Unparsed, 0));
        assert_eq!(spans("echo")[0].1, colour(Piece::Literal, 0));
    }

    #[test]
    fn an_empty_line_produces_nothing_to_paint() {
        assert!(spans("").is_empty());
    }

    /// 序号按出现顺序递增，与参数类型无关。
    #[test]
    fn positions_count_up_across_the_line() {
        use azalea_brigadier::prelude::*;

        let mut tree: CommandDispatcher<Source<Nothing>> = CommandDispatcher::new();
        tree.register(literal("add").then(
            argument("a", integer()).then(
                argument("b", integer()).then(
                    argument("c", integer()).executes(|_: &CommandContext<Source<Nothing>>| 1),
                ),
            ),
        ));

        let coloured: Vec<(String, Option<Color>)> =
            styled_line(&tree, &source(), &paint(), "add 1 2 3")
                .buffer
                .into_iter()
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(style, text)| (text, style.foreground))
                .collect();

        // 分隔空白归入前一段字面量。
        assert_eq!(coloured[0], ("add ".to_owned(), colour(Piece::Literal, 0)));
        assert_eq!(coloured[1], ("1".to_owned(), colour(Piece::Argument, 0)));
        assert_eq!(coloured[2], ("2".to_owned(), colour(Piece::Argument, 1)));
        assert_eq!(coloured[3], ("3".to_owned(), colour(Piece::Argument, 2)));
    }

    /// 着色闭包拿得到原文，于是「按内容上色」这种方案写得出来。
    #[test]
    fn the_closure_decides_everything() {
        let paint: Paint = Arc::new(|token: &Token| {
            if token.text.contains("你好") {
                Style::new().fg(Color::Blue)
            } else {
                Style::new()
            }
        });

        let coloured: Vec<(String, Option<Color>)> =
            styled_line(&dispatcher(), &source(), &paint, "echo 你好")
                .buffer
                .into_iter()
                .map(|(style, text)| (text, style.foreground))
                .collect();

        assert!(
            coloured.contains(&("你好".to_owned(), Some(Color::Blue))),
            "{coloured:?}"
        );
    }

    /// 中文、emoji 都必须能安全切段，不能切在字符中间。
    #[test]
    fn handles_non_ascii_without_panicking() {
        for line in [
            "echo 你好",
            "echo 你好 世界",
            "你好",
            "🎮",
            "echo café ☕",
            "ec 你好",
        ] {
            assert_roundtrips(line);
        }
    }

    #[test]
    fn a_non_ascii_argument_is_still_an_argument() {
        let spans = spans("echo 你好");
        assert_eq!(spans[0].1, colour(Piece::Literal, 0), "echo 是字面量");
        assert_eq!(spans[1].0, "你好");
        assert_eq!(spans[1].1, colour(Piece::Argument, 0));
    }

    #[test]
    fn preserves_input_exactly() {
        for line in [
            "echo hello",
            "  echo   hello  world  ",
            "ecoh",
            "",
            "echo 你好 世界",
        ] {
            assert_roundtrips(line);
        }
    }

    /// 幽灵文本插在光标处，且不改动真实内容。
    #[test]
    fn the_ghost_goes_where_the_cursor_is() {
        let styled = with_ghost(
            styled_line(&dispatcher(), &source(), &paint(), "echo "),
            5,
            "<message>",
            &paint(),
        );

        let (before, ghost): (String, String) = styled.buffer.iter().fold(
            (String::new(), String::new()),
            |(mut kept, mut ghost), (style, text)| {
                if style.foreground == colour(Piece::Ghost, 0) {
                    ghost.push_str(text);
                } else {
                    kept.push_str(text);
                }
                (kept, ghost)
            },
        );

        assert_eq!(before, "echo ", "真实内容不该被改动");
        assert_eq!(ghost, "<message>");
    }
}
