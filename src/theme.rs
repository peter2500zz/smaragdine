//! 谁该是什么颜色。
//!
//! 着色不是一张固定的表，而是一个闭包：控制台把每一小段交给你，你说它长什么
//! 样。于是「参数按出现顺序循环取色」只是默认实现的一种写法，换成按类型上色、
//! 按内容上色、或者干脆不上色，都不必动库。

use nu_ansi_term::{Color, Style};

/// 一行里的一小段是什么。
///
/// 注意它**不表示合法性也不表示类型**：指令名无论对错都是 [`Piece::Literal`]
/// —— 打错时之所以看着是红的，是因为整行都没被解析掉，落进了
/// [`Piece::Unparsed`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Piece {
    /// 字面量与它后面的空白 —— 已经解析掉、无需强调的部分。
    Literal,
    /// 一个参数。`index` 是它在这一行里的出现顺序，从 0 起。
    Argument,
    /// 解析不掉的那一截尾巴。
    Unparsed,
    /// 光标处的幽灵文本：选中候选的剩余部分，或是 `<参数名>` 这样的占位。
    Ghost,
    /// 输入行右侧那一句：这一行为什么不成立。
    Failure,
    /// 输入行右侧那一句：此处该填什么。
    Hint,
    /// 提示符的指示符（默认那个 `> `）。
    ///
    /// 只有颜色会被用到 —— reedline 的提示符接口收的是 `Color` 而不是
    /// `Style`，粗体下划线之类到不了那里。
    Prompt,
}

/// 交给着色闭包的一小段。
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Token<'a> {
    /// 这是哪一类。
    pub piece: Piece,
    /// 参数的出现顺序（从 0 起）。其余各类恒为 0。
    pub index: usize,
    /// 这一段的原文。
    pub text: &'a str,
}

impl<'a> Token<'a> {
    pub(crate) fn new(piece: Piece, index: usize, text: &'a str) -> Self {
        Self { piece, index, text }
    }
}

/// 着色闭包。
///
/// 在**每一次重绘**时对每一小段调用一遍，所以别在里面做重活。
pub type Paint = std::sync::Arc<dyn Fn(&Token) -> Style + Send + Sync>;

/// 参数依次取这五色，循环。
const ARGUMENTS: [Color; 5] = [
    Color::LightCyan,
    Color::LightYellow,
    Color::LightGreen,
    Color::LightPurple,
    Color::Yellow,
];

/// 默认配色，与 Minecraft 指令栏一致。
///
/// | 片段 | 颜色 |
/// |------|------|
/// | 字面量与空白 | 灰 |
/// | 参数 | 按**出现顺序**循环取青、黄、绿、品红、金 |
/// | 解析不掉的尾巴 | 红 |
/// | 幽灵文本、右侧提示 | 暗灰 |
/// | 右侧的出错原因 | 红 |
pub fn default_paint(token: &Token) -> Style {
    let color = match token.piece {
        // 对应 ANSI 37。nu-ansi-term 的 `LightGray` 是 97（亮白）反而更刺眼，
        // 它这里的 `White` 才是 37。
        Piece::Literal => Color::White,
        Piece::Argument => ARGUMENTS[token.index % ARGUMENTS.len()],
        Piece::Unparsed | Piece::Failure => Color::LightRed,
        Piece::Ghost | Piece::Hint => Color::DarkGray,
        Piece::Prompt => Color::Green,
    };
    Style::new().fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(piece: Piece, index: usize) -> Option<Color> {
        default_paint(&Token::new(piece, index, "x")).foreground
    }

    /// 指令名是灰的 —— 颜色不表示合法性。
    #[test]
    fn a_literal_is_grey() {
        assert_eq!(painted(Piece::Literal, 0), Some(Color::White));
    }

    /// 参数按出现顺序循环取色，与类型无关。
    #[test]
    fn arguments_cycle_through_the_palette() {
        assert_eq!(painted(Piece::Argument, 0), Some(ARGUMENTS[0]));
        assert_eq!(painted(Piece::Argument, 1), Some(ARGUMENTS[1]));
        assert_eq!(
            painted(Piece::Argument, ARGUMENTS.len()),
            Some(ARGUMENTS[0])
        );
    }

    /// 出错的东西是红的，说明性的东西是暗灰的。
    #[test]
    fn trouble_is_red_and_asides_are_dim() {
        assert_eq!(painted(Piece::Unparsed, 0), Some(Color::LightRed));
        assert_eq!(painted(Piece::Failure, 0), Some(Color::LightRed));
        assert_eq!(painted(Piece::Ghost, 0), Some(Color::DarkGray));
        assert_eq!(painted(Piece::Hint, 0), Some(Color::DarkGray));
    }

    /// 闭包能拿到原文，于是「按内容上色」这种方案也写得出来。
    #[test]
    fn a_custom_scheme_sees_the_text() {
        let paint: Paint = std::sync::Arc::new(|token: &Token| {
            if token.text.starts_with('-') {
                Style::new().fg(Color::Blue)
            } else {
                default_paint(token)
            }
        });

        let flag = Token::new(Piece::Argument, 0, "--force");
        assert_eq!(paint(&flag).foreground, Some(Color::Blue));
    }
}
