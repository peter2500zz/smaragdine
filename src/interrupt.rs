//! 两段式的 Ctrl-C。
//!
//! 与多数 shell 一致：行里有内容时 Ctrl-C 只是把它抹掉；空行上再按才提示
//! 「再按一次退出」，第三次落下才真的退出。
//!
//! 之所以要绕一圈，是因为 reedline 在 Ctrl-C 上会先把行清掉再返回信号
//! （`engine.rs` 的 `CtrlC` 分支里 `run_edit_commands(&[EditCommand::Clear])`），
//! 等 `read_line` 返回时已经无从判断按下那一刻行里有没有东西。高亮器每次重绘
//! 都会拿到当前行，就借它留一份影子。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use reedline::{Hinter, History};

use crate::{
    theme::{Paint, Piece, Token},
    util::lock,
};

/// 最近一次重绘时输入行的内容。
#[derive(Clone, Default)]
pub(crate) struct LineShadow(Arc<Mutex<String>>);

impl LineShadow {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 高亮器每次被调用时记下当前行。
    pub(crate) fn record(&self, line: &str) {
        let mut shadow = lock(&self.0);
        shadow.clear();
        shadow.push_str(line);
    }

    /// 按下 Ctrl-C 那一刻行里是不是空的。
    pub(crate) fn was_empty(&self) -> bool {
        lock(&self.0).trim().is_empty()
    }
}

/// 「再按一次退出」这个状态是否亮着。
#[derive(Clone, Default)]
pub(crate) struct ExitArmed(Arc<AtomicBool>);

impl ExitArmed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_armed(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn arm(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub(crate) fn disarm(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// 在空行末尾打出「再按一次退出」的灰字。
///
/// 借 `Hinter` 实现 —— 常规的历史补全提示我们没有用到，这个位置正好空着。
///
/// 注意 `Hinter` 有两条硬限制，选它是因为这里恰好都不碰：
///
/// * 它印在 `after_cursor` **之后**，永远在行尾，落不到光标处
///   （`painter.rs` 里 `Print(before_cursor) → SavePosition →
///   Print(after_cursor) → Print(hint)`）。
/// * 菜单一激活它就**根本不印** —— 那一段是
///   `if let Some(menu) = menu { print_menu } else { print hint }`。
///
/// 这条提示只在空行上出现，而空行时菜单本来就是关的，所以两条都不成问题。
/// 若要做光标处的幽灵文本（补全的剩余部分那种），得走 `Highlighter`：
/// `render_around_insertion_point` 是按字节走 styled 段、在光标处切开的，
/// 只要把幽灵段插在光标那个偏移上，它就落进 `after_cursor` 的最前面，
/// 紧挨着光标印出，且不受菜单影响。
pub(crate) struct ExitHint {
    armed: ExitArmed,
    /// 提示语本身 —— 库只在这几处开口，措辞归使用者。
    hint: String,
    paint: Paint,
}

impl ExitHint {
    pub(crate) fn new(armed: ExitArmed, hint: String, paint: Paint) -> Self {
        Self { armed, hint, paint }
    }
}

impl Hinter for ExitHint {
    fn handle(
        &mut self,
        line: &str,
        _pos: usize,
        _history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        if !self.armed.is_armed() {
            return String::new();
        }

        // 敲了任何东西就撤下提示 —— 用户显然是要接着输入，而不是要退出。
        if !line.is_empty() {
            self.armed.disarm();
            return String::new();
        }

        if use_ansi_coloring {
            (self.paint)(&Token::new(Piece::Hint, 0, &self.hint))
                .paint(&self.hint)
                .to_string()
        } else {
            self.hint.clone()
        }
    }

    fn complete_hint(&self) -> String {
        // 这条提示不是补全候选，不该被 Tab/右键之类的动作填进行里。
        String::new()
    }

    fn next_hint_token(&self) -> String {
        String::new()
    }
}

/// 一次 Ctrl-C 之后该做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interrupt {
    /// 行被抹掉了，继续待命。
    ClearedLine,
    /// 空行上的第一次 —— 亮出「再按一次退出」。
    Armed,
    /// 亮着的时候又按了一次 —— 退出。
    Exit,
}

/// 判定这次 Ctrl-C 落在三种情形里的哪一种，并更新提示状态。
pub(crate) fn on_interrupt(shadow: &LineShadow, armed: &ExitArmed) -> Interrupt {
    if !shadow.was_empty() {
        // reedline 已经把行清掉了，这里只需要把提示状态复位。
        armed.disarm();
        return Interrupt::ClearedLine;
    }

    if armed.is_armed() {
        Interrupt::Exit
    } else {
        armed.arm();
        Interrupt::Armed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hint(armed: ExitArmed) -> ExitHint {
        ExitHint::new(
            armed,
            crate::Text::default().exit_hint,
            std::sync::Arc::new(crate::theme::default_paint),
        )
    }

    #[test]
    fn a_non_empty_line_is_only_cleared() {
        let shadow = LineShadow::new();
        let armed = ExitArmed::new();
        shadow.record("echo hi");

        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::ClearedLine);
        assert!(!armed.is_armed(), "行里有内容时不该亮出退出提示");
    }

    #[test]
    fn an_empty_line_arms_then_exits() {
        let shadow = LineShadow::new();
        let armed = ExitArmed::new();
        shadow.record("");

        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::Armed);
        assert!(armed.is_armed());
        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::Exit);
    }

    /// 只有空白的行也算空 —— 敲了几个空格不该挡住退出。
    #[test]
    fn a_whitespace_only_line_counts_as_empty() {
        let shadow = LineShadow::new();
        let armed = ExitArmed::new();
        shadow.record("   ");

        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::Armed);
    }

    /// 亮着提示时又去输入，提示要撤下；此时的 Ctrl-C 回到「第一次」。
    #[test]
    fn typing_disarms_the_hint() {
        let shadow = LineShadow::new();
        let armed = ExitArmed::new();
        shadow.record("");
        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::Armed);

        let mut hint = test_hint(armed.clone());
        assert!(!render(&mut hint, "").is_empty(), "空行上该显示提示");
        assert!(render(&mut hint, "e").is_empty(), "一旦输入就该撤下");
        assert!(!armed.is_armed(), "撤下的同时状态也要复位");

        // 再清空行，又是第一次。
        shadow.record("");
        assert_eq!(on_interrupt(&shadow, &armed), Interrupt::Armed);
    }

    /// 没亮提示时不该凭空冒出灰字。
    #[test]
    fn nothing_is_shown_until_armed() {
        let armed = ExitArmed::new();
        let mut hint = test_hint(armed.clone());
        assert!(render(&mut hint, "").is_empty());

        armed.arm();
        assert!(!render(&mut hint, "").is_empty());
    }

    /// 这条提示不能被当成可补全的内容填进输入行。
    #[test]
    fn the_hint_is_never_completable() {
        let hint = test_hint(ExitArmed::new());
        assert!(hint.complete_hint().is_empty());
        assert!(hint.next_hint_token().is_empty());
    }

    /// 影子记的是最近一次重绘的内容，不是累积的。
    #[test]
    fn the_shadow_tracks_only_the_latest_line() {
        let shadow = LineShadow::new();
        shadow.record("echo hi");
        assert!(!shadow.was_empty());
        shadow.record("");
        assert!(shadow.was_empty());
    }

    fn render(hint: &mut ExitHint, line: &str) -> String {
        hint.handle(line, line.len(), &NoHistory, false, "")
    }

    /// Hinter 的签名要一个 History，我们的提示压根不看它。
    struct NoHistory;

    impl History for NoHistory {
        fn save(&mut self, h: reedline::HistoryItem) -> reedline::Result<reedline::HistoryItem> {
            Ok(h)
        }
        fn load(&self, _id: reedline::HistoryItemId) -> reedline::Result<reedline::HistoryItem> {
            unimplemented!()
        }
        fn count(&self, _query: reedline::SearchQuery) -> reedline::Result<i64> {
            Ok(0)
        }
        fn search(
            &self,
            _query: reedline::SearchQuery,
        ) -> reedline::Result<Vec<reedline::HistoryItem>> {
            Ok(Vec::new())
        }
        fn update(
            &mut self,
            _id: reedline::HistoryItemId,
            _updater: &dyn Fn(reedline::HistoryItem) -> reedline::HistoryItem,
        ) -> reedline::Result<()> {
            Ok(())
        }
        fn clear(&mut self) -> reedline::Result<()> {
            Ok(())
        }
        fn delete(&mut self, _id: reedline::HistoryItemId) -> reedline::Result<()> {
            Ok(())
        }
        fn sync(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn session(&self) -> Option<reedline::HistorySessionId> {
            None
        }
    }
}
