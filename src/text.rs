//! 控制台自己会说的那几句话。
//!
//! 库只在这几处开口，全都在这里，换成任何语言都行。指令执行失败时那些更细的
//! 说法来自 brigadier（`Unknown command`、`Invalid bool, expected true or
//! false but found 'x'` ……），它们是结构化的错误，渲染权在
//! [`ConsoleBuilder::on_error`] 那一头，不在这里。
//!
//! [`ConsoleBuilder::on_error`]: crate::ConsoleBuilder::on_error

/// 控制台会说的话。
///
/// 默认是英文，与 Minecraft 指令栏的措辞一致。
///
/// ```
/// # use smaragdine::Text;
/// let text = Text {
///     exit_hint: "  再按一次 Ctrl-C 退出".into(),
///     unknown_command: "不认识的指令，或还没打完".into(),
///     ..Text::default()
/// };
/// ```
///
/// 刻意**没有**标 `#[non_exhaustive]`：那会禁掉上面这种字面量写法。用
/// `..Text::default()` 收尾就行 —— 日后加字段也不会碰坏你的代码，只有把
/// 每个字段都写全的人才会被打断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// 空行上按下 Ctrl-C 后，行尾那句灰字。再按一次才真的退出。
    ///
    /// 前面留两个空格，免得贴着光标。
    pub exit_hint: String,
    /// 右侧那一句：连指令名都没认出来（或还没打完）。
    pub unknown_command: String,
    /// 右侧那一句：指令名认出来了，但后面的参数不对。
    pub incorrect_argument: String,
    /// 反向搜索（Ctrl-R）时提示符里的标签，默认提示符用。
    pub history_search: String,
    /// 反向搜索没找到东西时，加在标签前面的前缀，默认提示符用。
    pub history_search_failing: String,
    /// 指令体 panic 了、已被拦下。
    ///
    /// 一条指令写崩了不该把整个程序带走，所以库把它兜住 —— 但必须说一声，
    /// 否则用户只会觉得「这条指令按了没反应」。
    pub command_panicked: String,
    /// 起不了线程，这条指令改为同步执行（于是它跑完之前你敲不了下一条）。
    pub command_ran_inline: String,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            exit_hint: "  press Ctrl-C again to exit".to_owned(),
            // 这两句照抄游戏内指令栏。
            unknown_command: "Unknown or incomplete command".to_owned(),
            incorrect_argument: "Incorrect argument for command".to_owned(),
            history_search: "reverse-search".to_owned(),
            history_search_failing: "failing ".to_owned(),
            command_panicked: "the command panicked; the console caught it".to_owned(),
            command_ran_inline: "could not spawn a thread; ran the command inline".to_owned(),
        }
    }
}
