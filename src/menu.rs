//! 没有候选时不画出来的补全菜单。
//!
//! reedline 的 `IdeMenu` 在候选为空时会画一行硬编码的 `NO RECORDS FOUND`，
//! 那串字既不在 `MenuSettings` 里、也没有构建器可设。而我们要的是：没有
//! 候选就干脆不画。
//!
//! 好在「激活」与「显示」在 reedline 里本就是两回事：绘制取的是
//! `is_visible()` 而非 `is_active()`（`engine.rs` 里
//! `self.menus.iter().find(|menu| menu.is_visible())`），而 `is_visible()`
//! 的默认实现留了口子：
//!
//! ```ignore
//! fn is_visible(&self) -> bool {
//!     self.is_active() && !self.is_awaiting_first_answer()
//! }
//! ```
//!
//! 注释说得很明白：「An active menu still awaiting its first answer is not
//! [visible]: it takes input, but claims no indicator and reserves no rows.」
//! 也就是说，让菜单活着却不画是库本来就支持的用法。这里照此再加一条：没有
//! 候选时同样不画。菜单仍然接收输入、跟随编辑更新，只是不占屏幕。
//!
//! 「画没画出来」这件事还要往外说一声：按键往弹窗走还是往历史走，取决于
//! 它。见 [`MenuVisible`]。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use reedline::{Completer, Editor, IdeMenu, Menu, MenuBuilder, MenuEvent, Painter, Suggestion};

use crate::util::lock;

/// 菜单里选中的是第几条。
///
/// 由按键策略维护，不问菜单要 —— 菜单的选中位次要等到重绘时才更新，而
/// 语法高亮跑在那之前，问它只会拿到上一帧的值。实测过：按 ↓ 之后菜单已经
/// 高亮到第二条，幽灵文本还在预览第一条。
///
/// 记的是「移动了几次」而非绝对下标：候选数量要到高亮器现算出来才知道，
/// 由它按数量取模。
#[derive(Clone, Default)]
pub struct MenuCursor(Arc<Mutex<Cursor>>);

#[derive(Default)]
struct Cursor {
    /// 移动了几次。
    steps: isize,
    /// 被 Esc 收起过。收起之后不该再预览候选 —— 那时用户看不到自己在选
    /// 哪一条，一段说不清来历的灰字只会让人困惑。
    ///
    /// 这个状态不问菜单要（`MenuVisible` 要等重绘才更新，会慢一帧），而是
    /// 按键策略自己记：它本来就知道自己发的是收起还是打开。
    dismissed: bool,
}

impl MenuCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 换到相邻一条。
    pub fn step(&self, delta: isize) {
        lock(&self.0).steps += delta;
    }

    /// 回到第一条。菜单一重建（内容变了、重新打开）就该复位。
    pub fn reset(&self) {
        *lock(&self.0) = Cursor::default();
    }

    /// 菜单被收起了。
    pub fn dismiss(&self) {
        lock(&self.0).dismissed = true;
    }

    /// 菜单被 Esc 收起过吗。
    pub fn is_dismissed(&self) -> bool {
        lock(&self.0).dismissed
    }

    /// 在 `count` 条候选里，此刻选中第几条。
    pub fn index(&self, count: usize) -> Option<usize> {
        let cursor = lock(&self.0);
        (!cursor.dismissed && count > 0).then(|| cursor.steps.rem_euclid(count as isize) as usize)
    }
}

/// 补全菜单此刻有没有画在屏幕上。
///
/// 分派按键的地方是 `EditMode::parse_event`，而它看不见菜单，所以由菜单自己
/// 把这个事实发布出来。读到的是引擎处理完上一批事件之后的状态，也就是此刻
/// 屏幕上的样子 —— 正是分派按键时该看的那一刻。
#[derive(Clone, Default)]
pub struct MenuVisible(Arc<AtomicBool>);

impl MenuVisible {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn set(&self, visible: bool) {
        self.0.store(visible, Ordering::Relaxed);
    }

    /// 测试用：直接摆布可见性。
    ///
    /// 生产路径上只有 [`HidingMenu::publish`] 会写它 —— 「画没画出来」是菜单
    /// 自己才知道的事，别处不该有话语权。
    #[cfg(test)]
    pub(super) fn force(&self, visible: bool) {
        self.set(visible);
    }
}

/// 包住 `IdeMenu`，候选为空时不显示。
pub struct HidingMenu {
    inner: IdeMenu,
    visible: MenuVisible,
}

impl HidingMenu {
    pub fn new(name: &str, indicator: &str, visible: MenuVisible) -> Self {
        Self {
            inner: IdeMenu::default().with_name(name).with_marker(indicator),
            visible,
        }
    }

    /// 把当前可见性发布出去。
    ///
    /// 每个会改动菜单状态的方法末尾都调一次。之所以不挂在 `is_visible()` 上：
    /// 引擎持有的是 `ReedlineMenu`，它的 `impl Menu` **没有转发
    /// `is_visible`**，走的是默认公式，本类型的 `is_visible()` 根本不会被引擎
    /// 调用到（详见下面 `is_awaiting_first_answer` 的注释）。挂在状态变更处则
    /// 与引擎怎么查询无关，是确定的。
    fn publish(&self) {
        self.visible.set(self.is_visible());
    }
}

impl Menu for HidingMenu {
    /// 唯一改动：没有候选就不画。
    ///
    /// 挂在 `is_awaiting_first_answer` 而不是直接覆写 `is_visible`，是因为
    /// 引擎持有的是 `Vec<ReedlineMenu>`，而 `ReedlineMenu` 自己的 `impl Menu`
    /// **没有转发 `is_visible`** —— 它转发了 `results_are_provisional`、
    /// `is_awaiting_first_answer`、`set_cursor_pos`，唯独漏了这个，于是走的是
    /// 默认公式 `is_active() && !is_awaiting_first_answer()`，覆写 `is_visible`
    /// 根本不会被调用到。
    ///
    /// 而这个语义也对得上：reedline 给这个标志的注释是「takes input, but
    /// claims no indicator and reserves no rows」—— 正是我们要的「活着但不画」。
    fn is_awaiting_first_answer(&self) -> bool {
        self.inner.is_awaiting_first_answer() || self.inner.get_values().is_empty()
    }

    /// 直接调到本类型上时同样成立 —— 与上面那条保持一致。
    fn is_visible(&self) -> bool {
        self.is_active() && !self.is_awaiting_first_answer()
    }

    // ── 以下全部原样转发 ──
    //
    // 刻意不实现 `settings()`：`MenuSettings` 没有从 reedline 导出，外部
    // 根本没法命名它。它的默认实现会 panic，但只有 `name()` 与 `indicator()`
    // 的默认实现会去调它，而这两个都在下面覆写掉了 —— 引擎自身从不直接调
    // `settings()`（只调 `name()` 与 `indicator()`）。
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn indicator(&self) -> &str {
        self.inner.indicator()
    }

    /// 原样转发。
    ///
    /// 试过在无候选时一并报 false（本意是别让看不见的菜单吞掉按键），结果
    /// 菜单彻底不出来了：候选只在菜单被认作激活时才拉取（`engine.rs` 的编辑
    /// 处理里 `find(|m| m.is_active())` 之后才 `update_values`），让「激活」
    /// 反过来依赖「有候选」就成了死结。
    ///
    /// 按键那一头由 [`MenuVisible`] 解决 —— 分派时看的是「画没画出来」，
    /// 不必动这里。
    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn set_active(&mut self, active: bool) {
        self.inner.set_active(active);
        self.publish();
    }

    fn clear_input(&mut self) {
        self.inner.clear_input();
        self.publish();
    }

    fn menu_event(&mut self, event: MenuEvent) {
        self.inner.menu_event(event);
        self.publish();
    }

    fn can_quick_complete(&self) -> bool {
        self.inner.can_quick_complete()
    }

    fn can_partially_complete(
        &mut self,
        values_updated: bool,
        editor: &mut Editor,
        completer: &mut dyn Completer,
    ) -> bool {
        let completed = self
            .inner
            .can_partially_complete(values_updated, editor, completer);
        self.publish();
        completed
    }

    fn update_values(&mut self, editor: &mut Editor, completer: &mut dyn Completer) {
        self.inner.update_values(editor, completer);
        self.publish();
    }

    fn reset_position(&mut self) {
        self.inner.reset_position();
        self.publish();
    }

    fn update_working_details(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        painter: &Painter,
    ) {
        self.inner
            .update_working_details(editor, completer, painter);
        self.publish();
    }

    fn replace_in_buffer(&self, editor: &mut Editor) {
        self.inner.replace_in_buffer(editor);
    }

    /// 必须显式转发。
    ///
    /// 这个方法在 trait 上带默认实现（退回 `replace_in_buffer`），漏掉不会报错，
    /// 只会让 Tab 从「原地循环」悄悄退化成「采用一次然后不动」—— 与 `is_visible`
    /// 那个坑一模一样。
    fn replace_in_buffer_in_place(&self, editor: &mut Editor) {
        self.inner.replace_in_buffer_in_place(editor);
    }

    /// 同样必须显式转发。
    ///
    /// 与上面那条同理：trait 上带默认实现（返回 `None`），漏掉不会报错。区别
    /// 在于后果——引擎自己不调它（fork 里只有测试调），所以今天漏了也看不
    /// 出来。它是留给「想预览选中项」的调用方的，而这个包装的全部意义就是
    /// 「除了不画，其余原样」。
    fn selected_value(&self) -> Option<Suggestion> {
        self.inner.selected_value()
    }

    fn menu_required_lines(&self, terminal_columns: u16) -> u16 {
        self.inner.menu_required_lines(terminal_columns)
    }

    fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String {
        self.inner.menu_string(available_lines, use_ansi_coloring)
    }

    fn min_rows(&self) -> u16 {
        self.inner.min_rows()
    }

    fn get_values(&self) -> &[Suggestion] {
        self.inner.get_values()
    }

    fn results_are_provisional(&self) -> bool {
        self.inner.results_are_provisional()
    }

    fn set_cursor_pos(&mut self, pos: (u16, u16)) {
        self.inner.set_cursor_pos(pos);
        self.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(name: &str) -> (HidingMenu, MenuVisible) {
        menu_with_indicator(name, "| ")
    }

    fn menu_with_indicator(name: &str, indicator: &str) -> (HidingMenu, MenuVisible) {
        let visible = MenuVisible::new();
        (HidingMenu::new(name, indicator, visible.clone()), visible)
    }

    #[test]
    fn the_name_survives_the_wrapper() {
        let (menu, _) = menu("completion_menu");
        assert_eq!(menu.name(), "completion_menu");
    }

    #[test]
    fn the_indicator_survives_the_wrapper() {
        for indicator in ["/ ", ""] {
            let (menu, _) = menu_with_indicator("m", indicator);
            assert_eq!(menu.indicator(), indicator);
        }
    }

    /// 没激活时本来就不该显示。
    #[test]
    fn an_inactive_menu_is_not_visible() {
        let (menu, _) = menu("m");
        assert!(!menu.is_active());
        assert!(!menu.is_visible());
    }

    /// 一条候选都没有时不显示，但仍然算激活。
    ///
    /// 「仍然激活」是必须的：候选要等菜单被认作激活之后才会被拉取，若让激活
    /// 反过来依赖有候选，菜单就再也起不来了。
    #[test]
    fn an_empty_menu_is_hidden_but_still_active() {
        let (mut menu, _) = menu("m");
        menu.set_active(true);

        assert!(menu.get_values().is_empty());
        assert!(!menu.is_visible(), "没有候选就不该画出来");
        assert!(menu.is_active(), "仍须激活，否则永远拉不到候选");
    }

    /// 可见性要发布出去 —— 按键分派靠它决定往弹窗走还是往历史走。
    #[test]
    fn visibility_is_published_on_every_state_change() {
        let (mut menu, visible) = menu("m");
        assert!(!visible.get(), "初始未激活，不可见");

        // 激活但没有候选 —— 仍然不可见。
        menu.menu_event(MenuEvent::Activate(false));
        assert!(menu.is_active());
        assert!(!visible.get(), "空菜单不画出来，发布出去的也该是不可见");

        menu.menu_event(MenuEvent::Deactivate);
        assert!(!menu.is_active());
        assert!(!visible.get());
    }
}
