//! 控制台的按键策略。
//!
//! 分派分三层，与游戏里那个补全弹窗一致：弹窗可见时它独占 ↑↓ / Tab /
//! Shift+Tab / Esc 这几个键；弹窗不要的键才轮到编辑框；再不要才轮到历史
//! 走位。写成表就是：
//!
//! | 键 | 弹窗可见 | 弹窗收起 |
//! |----|---------|---------|
//! | ↑ / ↓ | 上下移动选中项 | 翻历史 |
//! | Tab | 采用选中项，再按则原地换下一条 | 打开弹窗 |
//! | Shift+Tab | 同上，反向 | 打开弹窗 |
//! | 光标移动 | 收起弹窗，再按编辑模式原义移动 | 按编辑模式原义移动 |
//! | Esc | 收起弹窗 | 清除选区 |
//! | Enter | 执行本行 | 执行本行 |
//!
//! reedline 这边对应的位置只有 `EditMode::parse_event` —— 它是唯一能按当前
//! 状态决定「这个键归谁」的地方。键位表做不到：`UntilFound` 里的 `MenuUp`
//! 只在**没有激活菜单**时才让路（`handle_editor_event` 的
//! `MenuUp` 分支：`active_menu().map_or(Inapplicable, ...)`），看的是 `is_active()` 而不是
//! 「画没画出来」，而我们的菜单在没有候选时正是活着但不画的。
//!
//! 把策略集中到这里，还顺带解决了另一件事：**菜单该在什么时候重新出现**。
//! 判据是内容变没变 —— 输入行的内容一改就重算补全。早先的写法是把
//! `0x20..=0x7e` 这 95 个字符逐个绑成「插入字符 + 开菜单」，于是退格、粘贴、
//! Ctrl-W、输入法送进来的中文全漏在外面：按 Esc 关掉菜单后再按退格，菜单
//! 就再也回不来了。现在改成对着**事件干了什么**判断。

use crossterm::event::{Event, KeyCode, KeyModifiers};
use reedline::{EditCommand, EditMode, PromptEditMode, ReedlineEvent, ReedlineRawEvent};

use crate::menu::{MenuCursor, MenuVisible};

/// 默认控制台不继承这些与 Minecraft 文本框无关的 Emacs/终端快捷键。
///
/// 这里只收紧库提供的默认值；调用方经 `ConsoleBuilder::edit_mode` 显式交进来的
/// 编辑模式一律照原样使用。
const DISABLED_CONTROL_CHARS: [char; 7] = ['l', 'r', 'o', 'b', 'f', 'p', 'n'];

pub(crate) fn default_edit_mode() -> reedline::Emacs {
    let mut bindings = reedline::default_emacs_keybindings();

    for key in DISABLED_CONTROL_CHARS {
        let removed = bindings.remove_binding(KeyModifiers::CONTROL, KeyCode::Char(key));
        debug_assert!(removed.is_some(), "reedline 不再默认绑定 Ctrl-{key}");
    }

    reedline::Emacs::new(bindings)
}

/// 控制台的按键分派。
pub(crate) struct ConsoleEditMode {
    /// 弹窗不要的键交给它 —— 相当于输入行本体。
    ///
    /// 默认以 reedline 的 Emacs 键位为底，只摘掉 Ctrl-L/R/O/B/F/P/N；想换 Vi
    /// 或自定义键位，整个换掉它即可。需要按状态分派的那几个键在 `route` 里
    /// 先拦掉了，轮不到这一层。
    inner: Box<dyn EditMode>,
    /// 补全菜单此刻画没画出来。
    menu_visible: MenuVisible,
    menu_name: String,
    /// 菜单里选中第几条。由这里维护而不是问菜单要：菜单的选中位次要等到
    /// 重绘时才更新，而语法高亮跑在那之前 —— 实测过，问它会慢一帧，按 ↓
    /// 之后菜单已经高亮到第二条，幽灵文本还在预览第一条。
    cursor: MenuCursor,
    /// 下一次 Tab 该不该先换一条候选，见 [`ConsoleEditMode::route`]。
    ///
    /// 随弹窗一起新建（初值 false），采用过一条候选后置为 true，↑↓ 手动
    /// 挑选后又置回 false。
    tab_cycles: bool,
}

impl ConsoleEditMode {
    pub(crate) fn new(
        menu_name: &str,
        menu_visible: MenuVisible,
        cursor: MenuCursor,
        inner: Box<dyn EditMode>,
    ) -> Self {
        Self {
            inner,
            menu_visible,
            menu_name: menu_name.to_owned(),
            cursor,
            tab_cycles: false,
        }
    }

    /// 弹窗与历史的分派。
    ///
    /// 返回 `None` 表示这个键弹窗不要，交给下一层。
    fn route(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<ReedlineEvent> {
        // 一批原始按键会先全部经过 parse_event，再由引擎逐条执行。Esc 已经
        // 在本地记成 dismissed 时，不能继续相信菜单上一帧发布的 visible。
        let visible = self.menu_visible.get() && !self.cursor.is_dismissed();

        Some(match (code, modifiers) {
            // ── 弹窗可见时独占的键 ──
            //
            // ↑↓ 是手动挑选，挑完就把循环状态收回去：紧接着的那次 Tab 应当
            // 采用你刚挑中的那条，而不是再往后跳一格。
            (KeyCode::Up, KeyModifiers::NONE) if visible => {
                self.tab_cycles = false;
                self.cursor.step(-1);
                ReedlineEvent::MenuUp
            }
            (KeyCode::Down, KeyModifiers::NONE) if visible => {
                self.tab_cycles = false;
                self.cursor.step(1);
                ReedlineEvent::MenuDown
            }
            // Tab 是「原地循环」：第一次只采用当前选中项，之后每一次都先换
            // 一条再采用。`MenuAccept` 是采用而不停用菜单，且对着算补全时的
            // 缓冲区套用，所以第二次是替换而非追加。
            (KeyCode::Tab, KeyModifiers::NONE) if visible => self.use_suggestion(false),
            // Shift+Tab 在多数终端上报成 BackTab，个别终端不带 SHIFT，所以不
            // 挑修饰键。语义是反向循环。
            (KeyCode::BackTab, _) if visible => self.use_suggestion(true),

            // ── 弹窗不要的键 ──
            //
            // 历史走位交给 reedline 自己的 PreviousHistory/NextHistory。它默认
            // 是前缀搜索，不是我们要的纯索引走位 —— 那件事在 history 模块里从
            // 源头解决了，这里不必也不该再打补丁。
            (KeyCode::Up, KeyModifiers::NONE) => self.dismiss_then(ReedlineEvent::PreviousHistory),
            (KeyCode::Down, KeyModifiers::NONE) => self.dismiss_then(ReedlineEvent::NextHistory),
            // 弹窗关着时 Tab 或 Shift+Tab 都把它打开；第一次只负责打开，
            // 方向要等菜单已经可见后才有意义。菜单已激活时这是空操作
            // （`handle_editor_event` 的 `Menu` 分支：`if self.active_menu().is_none()`），
            // 与「没有候选就没有弹窗」一致。
            (KeyCode::Tab, KeyModifiers::NONE) | (KeyCode::BackTab, _) => {
                // 新弹窗从「还没采用过」开始：循环状态随弹窗一起新建。
                self.tab_cycles = false;
                self.cursor.reset();
                ReedlineEvent::Menu(self.menu_name.clone())
            }

            // isEscape → hide()。菜单没开时这一下只是清掉选区，无害。
            (KeyCode::Esc, KeyModifiers::NONE) => {
                // 弹窗没了，连同它的循环状态一起 —— 收起就是丢掉，下次是
                // 全新的一份。
                self.dismiss();
                ReedlineEvent::Esc
            }

            // 弹窗**不该处理回车** —— 回车在指令控制台里只有一个意思：
            // 执行这一行。
            //
            // reedline 却把「回车 = 采用当前候选」写死在事件处理器里
            // （`handle_editor_event` 的 `Enter | Submit | SubmitOrNewline if
            // 有菜单激活`），键位绑定拦不住；而且它判的是 `is_active()`，我们
            // 那个不画出来的空菜单照样满足。于是先发一个 Esc 把菜单停掉，再让
            // 回车走它原本的提交路径。
            (KeyCode::Enter, KeyModifiers::NONE) => self.dismiss_then(ReedlineEvent::Enter),

            _ => return None,
        })
    }

    /// 丢掉当前补全会话在按键层的全部镜像。
    fn dismiss(&mut self) {
        self.tab_cycles = false;
        self.cursor.dismiss();
    }

    /// 先停用 reedline 里的菜单，再执行本来要做的事。
    fn dismiss_then(&mut self, event: ReedlineEvent) -> ReedlineEvent {
        self.dismiss();
        ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, event])
    }

    /// 内容变了就让补全菜单跟上。
    ///
    /// 菜单已经开着时，多发的这个 `Menu` 是空操作（`handle_editor_event` 的
    /// `Menu` 分支）；菜单被 Esc 关掉之后，它负责把菜单
    /// 请回来。
    ///
    /// **顺序是有讲究的**：`Menu` 排在编辑动作**前面** ——
    ///
    /// 先给许可，再由新内容拍板。reedline 这边的「拍板」就在编辑事件的处理
    /// 里：改完之后若整行空了，它会把菜单停掉（`handle_editor_event` 末尾那句 `line_buffer().get_buffer().is_empty()`）。把 `Menu`
    /// 放在后面，就等于在这个结论之后又强行把菜单请回来 —— 空行上于是常驻
    /// 一份列着全部指令的菜单，日志一滚就把屏幕挤满。
    fn refresh_menu(&mut self, event: ReedlineEvent) -> ReedlineEvent {
        if !changes_buffer(&event) {
            return event;
        }
        // 内容一变，弹窗就是重建出来的一份新的，循环从头开始。
        self.tab_cycles = false;
        self.cursor.reset();
        ReedlineEvent::Multiple(vec![ReedlineEvent::Menu(self.menu_name.clone()), event])
    }

    /// 采用当前选中项；若已经采用过一轮，则先换一条再采用。
    ///
    /// 第一次按 Tab 只采用当前选中项；此后每次都先换一条候选再采用，于是
    /// 连按 Tab 就在候选间循环，而不是把它们一条条摞进行里。
    fn use_suggestion(&mut self, backwards: bool) -> ReedlineEvent {
        let cycle = if backwards {
            ReedlineEvent::MenuUp
        } else {
            ReedlineEvent::MenuDown
        };

        if self.tab_cycles {
            self.cursor.step(if backwards { -1 } else { 1 });
        }
        let event = if self.tab_cycles {
            // 换一条再采用。两件事同批次送出没问题：`MenuAccept` 会先把这次
            // 移动落实，再取选中项（见 fork 里的 engine.rs 分支）。
            ReedlineEvent::Multiple(vec![cycle, ReedlineEvent::MenuAccept])
        } else {
            ReedlineEvent::MenuAccept
        };

        self.tab_cycles = true;
        event
    }
}

impl EditMode for ConsoleEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event: Event = event.into();

        if let Event::Key(key) = &event
            && let Some(routed) = self.route(key.code, key.modifiers)
        {
            return routed;
        }

        // 弹窗不要的键：先交给编辑器本体，再按事件的实际语义维护补全会话。
        // 粘贴（`Event::Paste`）也走这一路 —— reedline 会把它化成
        // `Edit([InsertString])`，于是同样被认作「内容变了」。
        match ReedlineRawEvent::try_from(event) {
            Ok(event) => {
                let resolved = self.inner.parse_event(event);
                if changes_buffer(&resolved) {
                    self.refresh_menu(resolved)
                } else if moves_cursor(&resolved) && !self.cursor.is_dismissed() {
                    // 保留 inner 的完整事件，而不是把 Left/Right 等重新手抄一
                    // 遍。这样自定义、Vi/Helix 以及带选择的移动仍按原义工作；
                    // Esc 只负责让菜单别吞掉其中的后备光标事件。
                    self.dismiss_then(resolved)
                } else {
                    resolved
                }
            }
            // 只有 `KeyEventKind::Release` 会被拒，reedline 本来就忽略它。
            Err(()) => ReedlineEvent::None,
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

/// 这个事件会不会改动输入行的内容。
fn changes_buffer(event: &ReedlineEvent) -> bool {
    match event {
        ReedlineEvent::Edit(commands) => commands.iter().any(changes_line),
        ReedlineEvent::Multiple(events) | ReedlineEvent::UntilFound(events) => {
            events.iter().any(changes_buffer)
        }
        _ => false,
    }
}

/// 这个事件是否包含光标/历史走位。
///
/// `UntilFound([MenuLeft, Left])` 是最重要的一例：不能只看外层事件，也不能
/// 丢掉菜单事件之后的后备动作。内容修改优先于这里；一个同时编辑并移动的
/// 复合事件应刷新新内容的补全，而不是把它关掉。
fn moves_cursor(event: &ReedlineEvent) -> bool {
    match event {
        ReedlineEvent::Edit(commands) => commands.iter().any(moves_cursor_command),
        ReedlineEvent::Multiple(events) | ReedlineEvent::UntilFound(events) => {
            events.iter().any(moves_cursor)
        }
        ReedlineEvent::PreviousHistory
        | ReedlineEvent::NextHistory
        | ReedlineEvent::Up
        | ReedlineEvent::Down
        | ReedlineEvent::Left
        | ReedlineEvent::Right
        | ReedlineEvent::ToStart
        | ReedlineEvent::ToEnd => true,
        _ => false,
    }
}

fn moves_cursor_command(command: &EditCommand) -> bool {
    let kind = command.edit_type();
    kind == EditCommand::MoveLeft { select: false }.edit_type()
        || kind == EditCommand::SelectAll.edit_type()
}

/// 一条编辑指令会不会改动内容。
///
/// 判据直接问 reedline 自己：`EditType` 里 `MoveCursor` 与 `NoOp` 不改内容，
/// 其余（`EditText`、`UndoRedo`）都改。
///
/// 之所以拿代表值比而不是写 `matches!`：`EditType` 没有从 crate 根导出
/// （`mod enums` 是私有模块），外部命不了名 —— 但它是 `PartialEq`，比较用不着
/// 名字。这里早先手抄过整张分类表，抄出来的东西不会跟着库走：`EditCommand`
/// 有一批分支挂在特性门后（reedline 的 `default = ["helix"]`），使用方的依赖图
/// 里只要有谁开了默认特性，`SelectLine` 与 `CopySelectionSystem` 就会冒出来，
/// 而手抄表里没有它们 —— 于是纯选中、纯复制会白白重开一次菜单。
fn changes_line(command: &EditCommand) -> bool {
    let kind = command.edit_type();

    kind != EditCommand::MoveLeft { select: false }.edit_type()  // MoveCursor { select: false }
        && kind != EditCommand::SelectAll.edit_type()            // MoveCursor { select: true }
        && kind != EditCommand::CopySelection.edit_type() // NoOp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    const MENU: &str = "completion_menu";

    fn mode(menu_visible: bool) -> ConsoleEditMode {
        let visible = MenuVisible::new();
        visible.force(menu_visible);
        ConsoleEditMode::new(
            MENU,
            visible,
            MenuCursor::new(),
            Box::new(default_edit_mode()),
        )
    }

    fn press(mode: &mut ConsoleEditMode, code: KeyCode, modifiers: KeyModifiers) -> ReedlineEvent {
        let raw = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(code, modifiers)))
            .expect("按下事件不该被拒");
        mode.parse_event(raw)
    }

    fn tap(mode: &mut ConsoleEditMode, code: KeyCode) -> ReedlineEvent {
        press(mode, code, KeyModifiers::NONE)
    }

    fn inner_event(code: KeyCode, modifiers: KeyModifiers) -> ReedlineEvent {
        let raw = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(code, modifiers)))
            .expect("按下事件不该被拒");
        reedline::Emacs::default().parse_event(raw)
    }

    fn edit_event(
        mode: &mut dyn EditMode,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> ReedlineEvent {
        let raw = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(code, modifiers)))
            .expect("按下事件不该被拒");
        mode.parse_event(raw)
    }

    #[test]
    fn the_default_mode_disables_only_the_selected_control_bindings() {
        let mut keys: Vec<_> = ('a'..='z').map(KeyCode::Char).collect();
        keys.extend([
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Enter,
            KeyCode::Tab,
        ]);

        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
        ] {
            for code in &keys {
                let mut reedline = reedline::Emacs::default();
                let mut console = default_edit_mode();
                let expected = edit_event(&mut reedline, *code, modifiers);
                let actual = edit_event(&mut console, *code, modifiers);
                let disabled = modifiers == KeyModifiers::CONTROL
                    && matches!(code, KeyCode::Char(key) if DISABLED_CONTROL_CHARS.contains(key));

                if disabled {
                    assert_ne!(expected, ReedlineEvent::None, "Ctrl-{code:?} 基线应有绑定");
                    assert_eq!(actual, ReedlineEvent::None, "Ctrl-{code:?} 应被禁用");
                } else {
                    assert_eq!(
                        actual, expected,
                        "Ctrl 组合 {modifiers:?}+{code:?} 不该改变"
                    );
                }
            }
        }
    }

    #[test]
    fn a_custom_edit_mode_keeps_its_own_control_bindings() {
        let visible = MenuVisible::new();
        let mut mode = ConsoleEditMode::new(
            MENU,
            visible,
            MenuCursor::new(),
            Box::new(reedline::Emacs::default()),
        );

        assert_eq!(
            press(&mut mode, KeyCode::Char('l'), KeyModifiers::CONTROL),
            ReedlineEvent::ClearScreen
        );
    }

    /// 弹窗可见时 ↑↓ 归弹窗。
    #[test]
    fn arrows_belong_to_the_popup_while_it_is_visible() {
        let mut mode = mode(true);
        assert_eq!(tap(&mut mode, KeyCode::Up), ReedlineEvent::MenuUp);
        assert_eq!(tap(&mut mode, KeyCode::Down), ReedlineEvent::MenuDown);
    }

    /// 弹窗关着时 ↑↓ 归历史。
    #[test]
    fn arrows_belong_to_history_while_the_popup_is_hidden() {
        let mut mode = mode(false);
        assert_eq!(
            tap(&mut mode, KeyCode::Up),
            ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, ReedlineEvent::PreviousHistory])
        );
        assert_eq!(
            tap(&mut mode, KeyCode::Down),
            ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, ReedlineEvent::NextHistory])
        );
    }

    fn cycle_then_accept(forward: bool) -> ReedlineEvent {
        let cycle = if forward {
            ReedlineEvent::MenuDown
        } else {
            ReedlineEvent::MenuUp
        };
        ReedlineEvent::Multiple(vec![cycle, ReedlineEvent::MenuAccept])
    }

    /// 原地循环：第一次 Tab 只采用当前选中项，之后每次都先换一条再
    /// 采用，于是连按 Tab 是在候选间循环，而不是把它们摞进行里。
    #[test]
    fn tab_accepts_first_then_cycles() {
        let mut mode = mode(true);
        assert_eq!(tap(&mut mode, KeyCode::Tab), ReedlineEvent::MenuAccept);
        assert_eq!(tap(&mut mode, KeyCode::Tab), cycle_then_accept(true));
        assert_eq!(tap(&mut mode, KeyCode::Tab), cycle_then_accept(true));
    }

    /// Shift+Tab 是同一件事的反向。
    #[test]
    fn shift_tab_cycles_backwards() {
        let mut mode = mode(true);
        assert_eq!(tap(&mut mode, KeyCode::BackTab), ReedlineEvent::MenuAccept);
        assert_eq!(tap(&mut mode, KeyCode::BackTab), cycle_then_accept(false));
        // 个别终端会带上 SHIFT 上报，同样要认。
        assert_eq!(
            press(&mut mode, KeyCode::BackTab, KeyModifiers::SHIFT),
            cycle_then_accept(false)
        );
    }

    /// ↑↓ 手动挑选之后，紧接着的 Tab 采用你刚挑中的那条，不再往后跳
    /// —— ↑↓ 会把循环状态收回去。
    #[test]
    fn arrows_reset_the_cycle() {
        for arrow in [KeyCode::Up, KeyCode::Down] {
            let mut mode = mode(true);
            tap(&mut mode, KeyCode::Tab); // 进入循环状态
            tap(&mut mode, arrow);
            assert_eq!(
                tap(&mut mode, KeyCode::Tab),
                ReedlineEvent::MenuAccept,
                "{arrow:?} 之后该采用选中项而非再跳一格"
            );
        }
    }

    /// 改动内容会重建弹窗，循环从头开始。
    #[test]
    fn editing_resets_the_cycle() {
        let mut mode = mode(true);
        tap(&mut mode, KeyCode::Tab);
        press(&mut mode, KeyCode::Char('x'), KeyModifiers::NONE);

        assert_eq!(tap(&mut mode, KeyCode::Tab), ReedlineEvent::MenuAccept);
    }

    /// Esc 把弹窗连同循环状态一起丢掉；下一次 Tab 重新打开，而不是采用
    /// 上一帧残留的候选。
    #[test]
    fn escape_resets_the_cycle() {
        let mut mode = mode(true);
        tap(&mut mode, KeyCode::Tab);
        tap(&mut mode, KeyCode::Esc);

        assert_eq!(
            tap(&mut mode, KeyCode::Tab),
            ReedlineEvent::Menu(MENU.to_owned())
        );
    }

    /// 弹窗关着时 Tab 与 Shift+Tab 都把它打开。
    #[test]
    fn tab_and_back_tab_open_a_hidden_menu() {
        let menu = ReedlineEvent::Menu(MENU.to_owned());
        assert_eq!(tap(&mut mode(false), KeyCode::Tab), menu);
        assert_eq!(tap(&mut mode(false), KeyCode::BackTab), menu);
        assert_eq!(
            press(&mut mode(false), KeyCode::BackTab, KeyModifiers::SHIFT),
            menu
        );
    }

    /// Esc 收起弹窗。
    #[test]
    fn escape_hides_the_popup() {
        assert_eq!(tap(&mut mode(true), KeyCode::Esc), ReedlineEvent::Esc);
    }

    /// 同一输入批次会先解析完所有按键、再交给引擎。Esc 后不能继续相信菜单
    /// 上一帧发布的 `visible = true`。
    #[test]
    fn escape_takes_effect_before_the_engine_handles_the_batch() {
        let mut history = mode(true);
        tap(&mut history, KeyCode::Esc);
        assert_eq!(
            tap(&mut history, KeyCode::Up),
            ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, ReedlineEvent::PreviousHistory])
        );

        let mut completion = mode(true);
        tap(&mut completion, KeyCode::Esc);
        assert_eq!(
            tap(&mut completion, KeyCode::Tab),
            ReedlineEvent::Menu(MENU.to_owned())
        );
    }

    /// 回车永远是提交，先把菜单停掉再走。
    ///
    /// reedline 判的是 `is_active()`，我们那个不画出来的空菜单照样满足，所以
    /// 弹窗看不见时这一步也不能省。
    #[test]
    fn enter_always_submits() {
        let expected = ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, ReedlineEvent::Enter]);
        assert_eq!(tap(&mut mode(true), KeyCode::Enter), expected);
        assert_eq!(tap(&mut mode(false), KeyCode::Enter), expected);
    }

    /// 改动内容的键要顺带把菜单请回来。
    ///
    /// 退格是这条规则最要紧的一例：早先按字符枚举的写法漏掉了它，Esc 关掉
    /// 菜单之后再按退格，菜单就再也不出现。
    #[test]
    fn edits_bring_the_menu_back() {
        let menu = ReedlineEvent::Menu(MENU.to_owned());
        let cases = [
            (KeyCode::Backspace, KeyModifiers::NONE),
            (KeyCode::Delete, KeyModifiers::NONE),
            (KeyCode::Char('e'), KeyModifiers::NONE),
            // 输入法送进来的中文 —— 不在任何 ASCII 区间里。
            (KeyCode::Char('你'), KeyModifiers::NONE),
            // Ctrl-W 删词、Ctrl-K 杀到行尾、Ctrl-U 删到行首。
            (KeyCode::Char('w'), KeyModifiers::CONTROL),
            (KeyCode::Char('k'), KeyModifiers::CONTROL),
            (KeyCode::Char('u'), KeyModifiers::CONTROL),
        ];

        for (code, modifiers) in cases {
            let event = press(&mut mode(false), code, modifiers);
            let ReedlineEvent::Multiple(parts) = &event else {
                panic!("{code:?}+{modifiers:?} 该带上开菜单事件: {event:?}");
            };
            // 开菜单排在编辑动作之前：编辑完若整行空了，reedline 会把菜单
            // 停掉，那个结论必须是最后一句话。
            assert_eq!(parts.first(), Some(&menu), "{code:?}+{modifiers:?}");
            assert_eq!(parts.len(), 2, "{code:?}+{modifiers:?}: {event:?}");
        }
    }

    /// 粘贴同样算改动内容。
    #[test]
    fn pasting_brings_the_menu_back() {
        let mut mode = mode(false);
        let raw = ReedlineRawEvent::try_from(Event::Paste("echo hi".to_owned()))
            .expect("粘贴事件不该被拒");
        let event = mode.parse_event(raw);

        let ReedlineEvent::Multiple(parts) = &event else {
            panic!("粘贴该带上开菜单事件: {event:?}");
        };
        assert_eq!(parts.first(), Some(&ReedlineEvent::Menu(MENU.to_owned())));
    }

    /// 光标移动采用底层编辑模式原本的语义，但先终止当前补全会话。特别是
    /// Left/Right 的底层事件本身含有 MenuLeft/MenuRight；菜单不先停用就会
    /// 吞掉真正的光标移动。
    #[test]
    fn moving_the_cursor_dismisses_completion_and_preserves_the_inner_event() {
        let cases = [
            (KeyCode::Left, KeyModifiers::NONE),
            (KeyCode::Right, KeyModifiers::NONE),
            (KeyCode::Home, KeyModifiers::NONE),
            (KeyCode::End, KeyModifiers::NONE),
            (KeyCode::Char('a'), KeyModifiers::CONTROL),
            (KeyCode::Char('e'), KeyModifiers::CONTROL),
        ];

        for (code, modifiers) in cases {
            let expected =
                ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, inner_event(code, modifiers)]);
            assert_eq!(
                press(&mut mode(true), code, modifiers),
                expected,
                "{code:?}+{modifiers:?}"
            );
        }
    }

    /// 补全已经停用后，普通移动不再重复发 Esc；否则 Shift+方向键建立的选区
    /// 会在每一步之前被清掉。
    #[test]
    fn moving_after_completion_was_dismissed_is_just_the_inner_event() {
        let mut mode = mode(true);
        tap(&mut mode, KeyCode::Esc);

        assert_eq!(
            tap(&mut mode, KeyCode::Home),
            inner_event(KeyCode::Home, KeyModifiers::NONE)
        );
    }

    /// 分类照抄自 reedline 的 `edit_type()`，抽查两侧各几条。
    #[test]
    fn the_classification_matches_reedlines_own() {
        for command in [
            EditCommand::InsertChar('a'),
            EditCommand::InsertString("hi".to_owned()),
            EditCommand::Backspace,
            EditCommand::Delete,
            EditCommand::Clear,
            EditCommand::CutWordLeft,
            EditCommand::KillLine,
            EditCommand::Complete,
            EditCommand::Undo,
            EditCommand::Redo,
            EditCommand::UppercaseWord,
            EditCommand::Paste,
        ] {
            assert!(changes_line(&command), "{command:?} 会改动内容");
        }

        for command in [
            EditCommand::MoveLeft { select: false },
            EditCommand::MoveToLineEnd { select: false },
            EditCommand::MoveWordRight { select: false },
            EditCommand::SelectAll,
            EditCommand::CopySelection,
            EditCommand::CopyWordLeft,
        ] {
            assert!(!changes_line(&command), "{command:?} 不改动内容");
        }
    }

    /// 嵌套事件里只要有一条改动内容就算改动。
    #[test]
    fn nested_events_are_inspected() {
        let edit = ReedlineEvent::Edit(vec![EditCommand::Backspace]);
        let moved = ReedlineEvent::Edit(vec![EditCommand::MoveLeft { select: false }]);

        assert!(changes_buffer(&ReedlineEvent::Multiple(vec![
            moved.clone(),
            edit.clone()
        ])));
        assert!(changes_buffer(&ReedlineEvent::UntilFound(vec![edit])));
        assert!(!changes_buffer(&ReedlineEvent::Multiple(vec![moved])));
        assert!(!changes_buffer(&ReedlineEvent::PreviousHistory));
    }
}
