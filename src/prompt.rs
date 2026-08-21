//! 控制台提示符。
//!
//! 默认那个刻意保持朴素：屏幕上的主角是日志与指令输出，提示符只需标出
//! 「这里可以输入」。想要别的（时间、当前状态、多段彩色前缀）就整个换掉它
//! —— 换掉之后右侧那一句仍然由控制台接管，见 [`ConsolePrompt`]。

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use nu_ansi_term::Color;
use reedline::{Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus};

use crate::{
    Text,
    theme::{Paint, Piece, Token},
    util::lock,
};

/// 输入行右侧那一句：出错原因，或此处该填什么。
///
/// 由高亮器在每次重绘时填、提示行读。之所以要绕这一道：`Prompt` 收的是
/// `&self`，自己看不到当前输入 —— 而高亮器是唯一每次重绘都拿得到整行的
/// 位置，且它本来就要解析这一行。
#[derive(Clone, Default)]
pub(crate) struct Aside(Arc<Mutex<Option<(String, Piece)>>>);

impl Aside {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set(&self, text: Option<(String, Piece)>) {
        *lock(&self.0) = text;
    }

    fn get(&self) -> Option<(String, Piece)> {
        lock(&self.0).clone()
    }
}

/// 控制台的提示符。
///
/// 使用者给了自己的 `Prompt` 时，除右侧之外一律让它说了算；右侧只在控制台
/// 有话要说（出错原因、该填什么）时才抢过来，其余时候仍归它。这条取舍是
/// 有意的：那一句是控制台对你正在输入的内容的唯一反馈，丢了它，换个提示符
/// 就等于把诊断也换掉了。
pub(crate) struct ConsolePrompt {
    aside: Aside,
    indicator: String,
    multiline_indicator: String,
    text: Text,
    paint: Paint,
    inner: Option<Box<dyn Prompt>>,
}

impl ConsolePrompt {
    pub(crate) fn new(
        aside: Aside,
        indicator: String,
        multiline_indicator: String,
        text: Text,
        paint: Paint,
        inner: Option<Box<dyn Prompt>>,
    ) -> Self {
        Self {
            aside,
            indicator,
            multiline_indicator,
            text,
            paint,
            inner,
        }
    }
}

impl Prompt for ConsolePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        match &self.inner {
            Some(inner) => inner.render_prompt_left(),
            None => Cow::Borrowed(""),
        }
    }

    /// 右侧那一句。行一长 reedline 会自动不画它（宽度不够就整块跳过），
    /// 所以不必自己算会不会撞上输入。
    fn render_prompt_right(&self) -> Cow<'_, str> {
        if let Some((text, piece)) = self.aside.get() {
            let style = (self.paint)(&Token::new(piece, 0, &text));
            return Cow::Owned(style.paint(text).to_string());
        }
        match &self.inner {
            Some(inner) => inner.render_prompt_right(),
            None => Cow::Borrowed(""),
        }
    }

    fn render_prompt_indicator(&self, mode: PromptEditMode) -> Cow<'_, str> {
        match &self.inner {
            Some(inner) => inner.render_prompt_indicator(mode),
            None => Cow::Borrowed(&self.indicator),
        }
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        match &self.inner {
            Some(inner) => inner.render_prompt_multiline_indicator(),
            None => Cow::Borrowed(&self.multiline_indicator),
        }
    }

    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        if let Some(inner) = &self.inner {
            return inner.render_prompt_history_search_indicator(search);
        }

        let prefix = match search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => &self.text.history_search_failing,
        };
        Cow::Owned(format!(
            "({prefix}{}: {}) ",
            self.text.history_search, search.term
        ))
    }

    fn get_indicator_color(&self) -> Color {
        match &self.inner {
            Some(inner) => inner.get_indicator_color(),
            None => (self.paint)(&Token::new(Piece::Prompt, 0, &self.indicator))
                .foreground
                .unwrap_or(Color::Green),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default_paint;

    fn prompt(inner: Option<Box<dyn Prompt>>) -> ConsolePrompt {
        ConsolePrompt::new(
            Aside::new(),
            "> ".to_owned(),
            "| ".to_owned(),
            Text::default(),
            Arc::new(default_paint),
            inner,
        )
    }

    #[test]
    fn the_multiline_indicator_is_configurable_or_hideable() {
        for indicator in ["... ", ""] {
            let prompt = ConsolePrompt::new(
                Aside::new(),
                "> ".to_owned(),
                indicator.to_owned(),
                Text::default(),
                Arc::new(default_paint),
                None,
            );

            assert_eq!(prompt.render_prompt_multiline_indicator(), indicator);
        }
    }

    #[test]
    fn the_default_multiline_indicator_stays_the_same() {
        assert_eq!(prompt(None).render_prompt_multiline_indicator(), "| ");
    }

    /// 右侧那一句由高亮器填，提示行照原样读出来。
    #[test]
    fn the_aside_shows_up_on_the_right() {
        let prompt = prompt(None);
        assert_eq!(prompt.render_prompt_right(), "");

        prompt
            .aside
            .set(Some(("<message> 要输出的内容".to_owned(), Piece::Hint)));
        assert!(
            prompt
                .render_prompt_right()
                .contains("<message> 要输出的内容"),
            "{:?}",
            prompt.render_prompt_right()
        );
    }

    /// 反向搜索的措辞取自文案表。
    #[test]
    fn the_history_search_label_comes_from_the_text_table() {
        let prompt = ConsolePrompt::new(
            Aside::new(),
            "> ".to_owned(),
            "| ".to_owned(),
            Text {
                history_search: "回溯".to_owned(),
                history_search_failing: "没找到 ".to_owned(),
                ..Text::default()
            },
            Arc::new(default_paint),
            None,
        );

        let search = |status| {
            prompt
                .render_prompt_history_search_indicator(PromptHistorySearch {
                    status,
                    term: "ec".to_owned(),
                })
                .to_string()
        };

        assert_eq!(search(PromptHistorySearchStatus::Passing), "(回溯: ec) ");
        assert_eq!(
            search(PromptHistorySearchStatus::Failing),
            "(没找到 回溯: ec) "
        );
    }

    struct Mine;

    impl Prompt for Mine {
        fn render_prompt_left(&self) -> Cow<'_, str> {
            Cow::Borrowed("[mine]")
        }
        fn render_prompt_right(&self) -> Cow<'_, str> {
            Cow::Borrowed("12:00")
        }
        fn render_prompt_indicator(&self, _: PromptEditMode) -> Cow<'_, str> {
            Cow::Borrowed("$ ")
        }
        fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
            Cow::Borrowed(":: ")
        }
        fn render_prompt_history_search_indicator(&self, _: PromptHistorySearch) -> Cow<'_, str> {
            Cow::Borrowed("(search) ")
        }
    }

    /// 换了提示符，除右侧之外全归它。
    #[test]
    fn a_custom_prompt_takes_over() {
        let prompt = prompt(Some(Box::new(Mine)));

        assert_eq!(prompt.render_prompt_left(), "[mine]");
        assert_eq!(
            prompt.render_prompt_indicator(PromptEditMode::Default),
            "$ "
        );
        assert_eq!(prompt.render_prompt_multiline_indicator(), ":: ");
        // 控制台没话说时，右侧也归它。
        assert_eq!(prompt.render_prompt_right(), "12:00");
    }

    /// 控制台有话说时抢过右侧 —— 那是它对你正在输入的内容的唯一反馈。
    #[test]
    fn the_console_takes_the_right_side_when_it_has_something_to_say() {
        let prompt = prompt(Some(Box::new(Mine)));
        prompt.aside.set(Some((
            "Unknown or incomplete command".to_owned(),
            Piece::Failure,
        )));

        assert!(
            prompt
                .render_prompt_right()
                .contains("Unknown or incomplete command"),
            "{:?}",
            prompt.render_prompt_right()
        );
    }
}
