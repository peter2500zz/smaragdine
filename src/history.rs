//! 控制台的指令历史。
//!
//! reedline 的 ↑↓ 默认不是「上一条 / 下一条」，而是 fish、zsh 那套**前缀
//! 搜索**：缓冲区非空且光标在行尾时，只翻得出以当前输入为前缀的历史
//! （`engine.rs` 的 `get_history_navigation_based_on_line_buffer`）。
//!
//! ```ignore
//! if self.editor.is_empty() || !self.editor.is_cursor_at_buffer_end() {
//!     HistoryNavigationQuery::Normal(...)              // 普通上下走
//! } else {
//!     HistoryNavigationQuery::PrefixSearch(buffer)     // 前缀搜索
//! }
//! ```
//!
//! 我们要的不是这样，而是「上一条 / 下一条」的纯索引走位：位置在
//! `[0, 条数]` 之间夹紧，与行里有什么完全无关。
//!
//! 差别在指令控制台里很致命：`echo hello wor` 不是任何历史条目的前缀，↑ 便
//! 毫无反应 —— 而且越往行尾（参数、greedy string 那一段）越是如此，看起来
//! 就像「只有空行才能翻历史」。
//!
//! 修法不在键位上。键位怎么绑都改变不了 `PreviousHistory` 落地之后的语义，
//! 而 reedline 也没给这个选择留任何开关。真正该说清楚的是一件事实：**本
//! 控制台的历史不做前缀过滤**。把这句话放进 `History::search`，reedline 自带
//! 的导航就退化成纯索引走位，连草稿的暂存与取回都一并对上了 ——
//! `update_buffer_from_history` 在游标走出历史区间时，会把
//! `PrefixSearch(prefix)` 里存着的原始缓冲区放回来，那正是我们要的草稿。

use reedline::{
    CommandLineSearch, History, HistoryItem, HistoryItemId, HistorySessionId, Result, SearchQuery,
};

/// 包住任意一份历史，只改一件事：查询时摘掉前缀条件。
///
/// 存储照旧由被包住的那一份负责 —— 默认是 reedline 的 `FileBackedHistory`
/// 不带文件（即纯内存），换成带文件的、或任何 `impl History` 都行，↑↓ 的
/// 走位语义不受影响。
pub(crate) struct ConsoleHistory {
    inner: Box<dyn History>,
}

impl ConsoleHistory {
    pub(crate) fn new(inner: Box<dyn History>) -> Self {
        Self { inner }
    }
}

/// 摘掉前缀条件 —— 这是本模块存在的全部理由。
///
/// 只动 `Prefix`。`Substring` 是反向搜索（Ctrl-R）用的，那种场景下用户明确
/// 是在按内容找东西，过滤正是他要的。
fn without_prefix_filter(mut query: SearchQuery) -> SearchQuery {
    if matches!(
        query.filter.command_line,
        Some(CommandLineSearch::Prefix(_))
    ) {
        query.filter.command_line = None;
    }
    query
}

impl History for ConsoleHistory {
    fn search(&self, query: SearchQuery) -> Result<Vec<HistoryItem>> {
        self.inner.search(without_prefix_filter(query))
    }

    /// 与 `search` 用同一套条件，否则「有几条」和「翻得到几条」会对不上。
    fn count(&self, query: SearchQuery) -> Result<i64> {
        self.inner.count(without_prefix_filter(query))
    }

    // ── 以下全部原样转发 ──
    fn save(&mut self, item: HistoryItem) -> Result<HistoryItem> {
        self.inner.save(item)
    }

    fn load(&self, id: HistoryItemId) -> Result<HistoryItem> {
        self.inner.load(id)
    }

    fn update(
        &mut self,
        id: HistoryItemId,
        updater: &dyn Fn(HistoryItem) -> HistoryItem,
    ) -> Result<()> {
        self.inner.update(id, updater)
    }

    fn clear(&mut self) -> Result<()> {
        self.inner.clear()
    }

    fn delete(&mut self, id: HistoryItemId) -> Result<()> {
        self.inner.delete(id)
    }

    fn sync(&mut self) -> std::io::Result<()> {
        self.inner.sync()
    }

    fn session(&self) -> Option<HistorySessionId> {
        self.inner.session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{SearchDirection, SearchFilter};

    fn filled(lines: &[&str]) -> ConsoleHistory {
        let mut history = ConsoleHistory::new(Box::new(reedline::FileBackedHistory::default()));
        for line in lines {
            history
                .save(HistoryItem::from_command_line(*line))
                .expect("内存历史不该写失败");
        }
        history
    }

    fn walk_back(history: &ConsoleHistory, typed: &str) -> Vec<String> {
        // 复刻 HistoryCursor 的走法：每次带上「不要与当前这条相同」，
        // 从上一条的 id 继续往回找。
        let mut found = Vec::new();
        let mut start_id = None;
        loop {
            let items = history
                .search(SearchQuery {
                    start_id,
                    end_id: None,
                    start_time: None,
                    end_time: None,
                    direction: SearchDirection::Backward,
                    limit: Some(1),
                    filter: SearchFilter::from_text_search(
                        CommandLineSearch::Prefix(typed.to_owned()),
                        None,
                    ),
                })
                .expect("查询不该失败");
            let Some(item) = items.into_iter().next() else {
                return found;
            };
            start_id = item.id;
            found.push(item.command_line);
        }
    }

    /// 核心：前缀条件必须被摘掉，否则行中输入根本翻不动历史。
    #[test]
    fn history_is_not_filtered_by_what_is_typed() {
        let history = filled(&["echo one", "echo two", "stop"]);

        // 「echo hello wor」不是任何一条的前缀 —— 原封不动交给 reedline 的话
        // 一条都翻不出来，这正是 ↑ 在行中失效的原因。
        assert_eq!(
            walk_back(&history, "echo hello wor"),
            vec!["stop", "echo two", "echo one"],
        );
    }

    /// 空行与整行内容走的是同一条路，结果必须一致。
    #[test]
    fn the_walk_is_the_same_whatever_is_typed() {
        let history = filled(&["echo one", "echo two", "stop"]);
        let expected = vec!["stop", "echo two", "echo one"];

        for typed in ["", "e", "echo", "echo hello wor", "你好", "zzz"] {
            assert_eq!(walk_back(&history, typed), expected, "typed = {typed:?}");
        }
    }

    /// 反向搜索（Ctrl-R）的子串过滤不能一起摘掉 —— 那种场景下过滤才是本意。
    #[test]
    fn substring_search_keeps_its_filter() {
        let history = filled(&["echo one", "stop"]);
        let items = history
            .search(SearchQuery {
                start_id: None,
                end_id: None,
                start_time: None,
                end_time: None,
                direction: SearchDirection::Backward,
                limit: None,
                filter: SearchFilter::from_text_search(
                    CommandLineSearch::Substring("sto".to_owned()),
                    None,
                ),
            })
            .expect("查询不该失败");

        let lines: Vec<&str> = items.iter().map(|i| i.command_line.as_str()).collect();
        assert_eq!(lines, vec!["stop"]);
    }

    /// count 与 search 必须口径一致。
    #[test]
    fn count_agrees_with_search() {
        let history = filled(&["echo one", "echo two", "stop"]);
        let query = |limit| SearchQuery {
            start_id: None,
            end_id: None,
            start_time: None,
            end_time: None,
            direction: SearchDirection::Backward,
            limit,
            filter: SearchFilter::from_text_search(
                CommandLineSearch::Prefix("zzz".to_owned()),
                None,
            ),
        };

        let found = history.search(query(None)).expect("查询不该失败").len();
        let counted = history.count(query(None)).expect("计数不该失败");
        assert_eq!(found, 3);
        assert_eq!(counted, 3);
    }
}
