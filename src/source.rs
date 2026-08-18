//! 指令执行时能拿到的东西。
//!
//! brigadier 的指令树泛型于「源」（`CommandDispatcher<S>`），每条指令体拿到
//! 的 `ctx.source` 就是那个 S。本模块把它定下来：你的上下文、输出接受器，
//! 外加一条留给控制台主循环的退出意向 —— 指令体够不着那个循环，只能留个
//! 意向让循环自己去读。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{printer::Printer, util::lock};

/// 你的程序交给指令使用的那一份东西。
///
/// 实现它的通常是一个装着句柄的结构体（配置、连接池、状态快照的入口……）。
/// 它被指令共享，所以要 `Send + Sync`；里面放 `Arc` 是常态。
///
/// ```
/// struct App {
///     rows: std::sync::atomic::AtomicU64,
/// }
///
/// enum Bye {
///     Stop(i32),
///     Restart,
/// }
///
/// impl smaragdine::Context for App {
///     type Exit = Bye;
/// }
/// ```
pub trait Context: Send + Sync + 'static {
    /// 指令请求退出时留下的意向，由你定义。
    ///
    /// 控制台主循环会把它原样交还给你（[`Exit::Quit`]），至于「优雅关停」
    /// 还是「立刻重启」是什么意思，库不做解释。
    ///
    /// [`Exit::Quit`]: crate::Exit::Quit
    type Exit: Send + 'static;
}

/// 指令执行时能拿到的东西，也就是 brigadier 那个 `S`。
///
/// 克隆廉价（一个 `Arc`）—— 必须如此：每解析一次输入都会克隆它，而解析发生
/// 在每一次击键上。
pub struct Source<C: Context> {
    inner: Arc<Inner<C>>,
}

struct Inner<C: Context> {
    context: C,
    printer: Printer,
    /// 指令请求的退出意向；控制台主循环据此收尾。
    exit: Mutex<Option<C::Exit>>,
    /// 置位后 reedline 的 `read_line` 会立刻返回。
    ///
    /// 指令跑在别的线程上，而控制台此刻正卡在 `read_line` 里等按键 ——
    /// 不把它叫出来，`stop` 得等你再敲一下才生效。
    interrupt: Arc<AtomicBool>,
}

impl<C: Context> Source<C> {
    /// 造一个源。
    ///
    /// 控制台会自己造，这个构造器是留给测试与「不开控制台、直接向指令树
    /// 执行一行」的场合的：那种时候输出落 stdout。
    pub fn new(context: C) -> Self {
        Self::with_printer(context, Printer::new())
    }

    pub(crate) fn with_printer(context: C, printer: Printer) -> Self {
        Self {
            inner: Arc::new(Inner {
                context,
                printer,
                exit: Mutex::new(None),
                interrupt: Arc::default(),
            }),
        }
    }

    /// 你的上下文。
    ///
    /// 注意解析也拿得到它：`requires` 与自定义候选提供者在**每一次击键**
    /// 时、在**控制台线程**上跑，与正在执行的指令并发。所以那两处只该读
    /// 廉价状态（原子量、快照）—— 取一把可能被指令长期持有的锁，会让输入
    /// 在那段时间里冻住。
    pub fn context(&self) -> &C {
        &self.inner.context
    }

    /// 输出接受器：写进去的东西打在提示行上方，不会把正在编辑的那一行搅乱。
    pub fn printer(&self) -> &Printer {
        &self.inner.printer
    }

    /// 请求退出，并把控制台从等待按键中叫醒。
    ///
    /// 先到先得 —— 一次退出只该有一个意向，后来的调用会被忽略。
    pub fn request_exit(&self, exit: C::Exit) {
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
    pub(crate) fn take_exit(&self) -> Option<C::Exit> {
        lock(&self.inner.exit).take()
    }

    /// 交给 reedline 的中断标志。
    pub(crate) fn interrupt_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.interrupt)
    }
}

impl<C: Context> Clone for Source<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<C: Context> std::fmt::Debug for Source<C> {
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

    impl Context for App {
        type Exit = Bye;
    }

    fn source() -> Source<App> {
        Source::new(App { name: "测试" })
    }

    #[test]
    fn the_context_is_always_there() {
        assert_eq!(source().context().name, "测试");
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
