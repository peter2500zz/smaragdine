//! 指令执行时能拿到的东西。
//!
//! brigadier 的指令树泛型于「源」（`CommandDispatcher<S>`），每条指令体拿到
//! 的 `ctx.source` 就是那个 S。本模块把它定下来：你的状态、输出接受器，
//! 外加一条留给控制台主循环的退出意向 —— 指令体够不着那个循环，只能留个
//! 意向让循环自己去读。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{printer::Printer, util::lock};

/// 指令执行时能拿到的东西，也就是 brigadier 那个 `S`。
///
/// 克隆廉价（一个 `Arc`）—— 必须如此：每解析一次输入都会克隆它，而解析发生
/// 在每一次击键上。
///
/// `S` 是你的应用状态，什么包装都可以；`R` 是指令请求退出时留下的原因。
/// 交给 [`crate::Console`] 时状态会跨线程共享，须满足 `Send + Sync + 'static`，
/// 但不必 `Clone`：`Source` 自己已经负责共享它。
///
/// ```
/// use smaragdine::Source;
/// use std::sync::Arc;
///
/// struct App;
/// enum Shutdown { Stop, Restart }
///
/// let state = Arc::new(App);
/// let source: Source<Arc<App>, Shutdown> = Source::new(Arc::clone(&state));
///
/// assert!(Arc::ptr_eq(source.state(), &state));
/// ```
pub struct Source<S, R = ()> {
    inner: Arc<Inner<S, R>>,
}

struct Inner<S, R> {
    state: S,
    printer: Printer,
    /// 指令请求的退出意向；控制台主循环据此收尾。
    exit: Mutex<Option<R>>,
    /// 置位后 reedline 的 `read_line` 会立刻返回。
    ///
    /// 指令跑在别的线程上，而控制台此刻正卡在 `read_line` 里等按键 ——
    /// 不把它叫出来，`stop` 得等你再敲一下才生效。
    interrupt: Arc<AtomicBool>,
}

impl<S, R> Source<S, R> {
    /// 造一个源。
    ///
    /// 控制台会自己造，这个构造器是留给测试与「不开控制台、直接向指令树
    /// 执行一行」的场合的：那种时候输出落 stdout。
    pub fn new(state: S) -> Self {
        Self::with_printer(state, Printer::new())
    }

    pub(crate) fn with_printer(state: S, printer: Printer) -> Self {
        Self {
            inner: Arc::new(Inner {
                state,
                printer,
                exit: Mutex::new(None),
                interrupt: Arc::default(),
            }),
        }
    }

    /// 你的应用状态。
    ///
    /// 注意解析也拿得到它：`requires` 与自定义候选提供者在**每一次击键**
    /// 时、在**控制台线程**上跑，与正在执行的指令并发。所以那两处只该读
    /// 廉价状态（原子量、快照）—— 取一把可能被指令长期持有的锁，会让输入
    /// 在那段时间里冻住。
    pub fn state(&self) -> &S {
        &self.inner.state
    }

    /// 输出接受器：写进去的东西打在提示行上方，不会把正在编辑的那一行搅乱。
    pub fn printer(&self) -> &Printer {
        &self.inner.printer
    }

    /// 请求退出，并把控制台从等待按键中叫醒。
    ///
    /// 先到先得 —— 一次退出只该有一个意向，后来的调用会被忽略。
    pub fn request_exit(&self, exit: R) {
        let mut slot = lock(&self.inner.exit);
        if slot.is_none() {
            *slot = Some(exit);
        }
        // 把控制台从 read_line 里叫出来，否则要等用户再敲一下才收尾。
        self.inner.interrupt.store(true, Ordering::Relaxed);
    }

    /// 有指令请求过退出吗。
    pub fn exit_requested(&self) -> bool {
        lock(&self.inner.exit).is_some()
    }

    /// 取走退出意向。控制台主循环每轮读一次。
    pub(crate) fn take_exit(&self) -> Option<R> {
        lock(&self.inner.exit).take()
    }

    /// 交给 reedline 的中断标志。
    pub(crate) fn interrupt_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.interrupt)
    }
}

impl<S> Source<S> {
    /// 请求退出，不附带额外原因。
    ///
    /// 使用自定义退出原因的控制台走 [`Self::request_exit`]。
    pub fn request_quit(&self) {
        self.request_exit(());
    }
}

impl<S, R> Clone for Source<S, R> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<S, R> std::fmt::Debug for Source<S, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("exit_requested", &self.exit_requested())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct App {
        name: &'static str,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Bye {
        Stop(i32),
        Restart,
    }

    fn source() -> Source<App, Bye> {
        Source::new(App { name: "测试" })
    }

    #[test]
    fn the_state_is_always_there() {
        assert_eq!(source().state().name, "测试");
    }

    struct Wrapped<T>(T);

    /// 状态只受线程安全约束，不受包装形状约束；库不该再为每种容器补实现。
    #[test]
    fn arbitrary_state_wrappers_need_no_library_trait() {
        let state = Arc::new(Wrapped(Arc::new(App { name: "嵌套" })));
        let source: Source<_, Bye> = Source::new(Arc::clone(&state));

        assert!(Arc::ptr_eq(source.state(), &state));
        assert_eq!(source.state().0.name, "嵌套");
    }

    #[test]
    fn the_default_exit_reason_has_a_quit_shortcut() {
        let source = Source::new(App { name: "默认" });
        source.request_quit();

        assert_eq!(source.take_exit(), Some(()));
    }

    /// 一次退出只该有一个意向：先到先得。
    #[test]
    fn the_first_exit_request_wins() {
        let source = source();
        source.request_exit(Bye::Restart);
        source.request_exit(Bye::Stop(1));

        assert_eq!(source.take_exit(), Some(Bye::Restart));
    }

    /// 退出意向要能把卡在 read_line 里的控制台叫醒，否则 stop 得等你再敲
    /// 一下才生效。
    #[test]
    fn requesting_an_exit_raises_the_interrupt() {
        let source = source();
        let interrupt = source.interrupt_flag();
        assert!(!interrupt.load(Ordering::Relaxed));

        source.request_exit(Bye::Stop(0));
        assert!(interrupt.load(Ordering::Relaxed));
    }

    #[test]
    fn taking_the_exit_empties_it() {
        let source = source();
        assert_eq!(source.take_exit(), None);

        source.request_exit(Bye::Stop(2));
        assert!(source.exit_requested());
        assert_eq!(source.take_exit(), Some(Bye::Stop(2)));
        assert!(!source.exit_requested());
    }

    /// 指令跑在别的线程上，拿的是克隆件 —— 它留下的意向必须回得到主循环。
    #[test]
    fn a_clone_shares_everything() {
        let source = source();
        let handed_to_a_command = source.clone();

        std::thread::spawn(move || handed_to_a_command.request_exit(Bye::Restart))
            .join()
            .expect("线程不该 panic");

        assert_eq!(source.take_exit(), Some(Bye::Restart));
    }
}
