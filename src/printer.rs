//! 后台输出的接受器。
//!
//! 控制台占着终端 —— 它在不断重绘提示行、补全菜单与右侧那一句。别处直接
//! 写 stdout 的字会插进这些控制序列中间，把屏幕搅烂。reedline 给的解法是
//! `ExternalPrinter`：一个有界通道，`read_line` 在等按键的间隙把它排空 ——
//! 先擦掉提示行、打完消息、再把提示行连同你正在编辑的内容重画出来。
//!
//! 于是本模块只做一件事：把那个通道包成一个随处可写的句柄，并保证它在三种
//! 状态下都做对的事。写进去的是日志、是一句话、还是 JSON，本模块没有意见
//! —— 格式、文件、级别一概是使用者的事。

use std::{
    fmt::Display,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use crossbeam::channel::Sender;

use crate::util::lock;

/// 一个写进去就不会把提示行搅乱的接受器。
///
/// 克隆廉价、`Send + Sync`、写入不阻塞，随便克隆到任何线程或任务里去写。
///
/// | 状态 | 行为 |
/// |------|------|
/// | 控制台跑着 | 经 `ExternalPrinter` 打在提示行上方 |
/// | 控制台没起 / 已退出 | 直接写 stdout |
/// | 通道满了 | 丢弃这一条，不阻塞写入方 |
///
/// 最后两条都不是随手定的：消息若投进一个没人再读的通道就石沉大海，而那
/// 恰恰是控制台起不来时最需要看到的一条；写入方则可能是业务线程，卡在这里
/// 等于让终端拖垮业务。
///
/// 接日志框架不需要任何 feature，它 impl 了 [`io::Write`]：
///
/// ```no_run
/// # let printer = smaragdine::Printer::new();
/// # fn init(_: impl for<'a> FnMut() -> smaragdine::Printer) {}
/// // tracing-subscriber 对「返回 writer 的闭包」有现成的 MakeWriter 实现
/// init(move || printer.clone());
/// ```
pub struct Printer {
    /// 控制台跑着时是 `Some`。由 [`Console::run`] 在起止两头拨。
    ///
    /// [`Console::run`]: crate::Console::run
    channel: Arc<Mutex<Option<Sender<String>>>>,
    /// `io::Write` 那一路的暂存。
    ///
    /// 日志框架写一条事件会调好几次 `write`，攒到 flush 或 drop 时一次性
    /// 交出去，才是完整的一行。克隆出来的句柄各攒各的，所以 [`Clone`] 不
    /// 复制它 —— 复制了会让半行内容凭空多出一份。
    pending: Vec<u8>,
}

impl Printer {
    /// 造一个接受器。
    ///
    /// 可以先于控制台存在 —— 通常的顺序正是先起日志、后起控制台。此刻写进
    /// 来的东西落 stdout，控制台一起来就自动改道。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打一行。
    ///
    /// 结尾的换行有没有都行，不会打出空行。
    pub fn print(&self, line: impl Display) {
        self.emit(&line.to_string());
    }

    /// 控制台起来了，之后的输出改投 external printer。
    pub(crate) fn attach(&self, sender: Sender<String>) {
        *lock(&self.channel) = Some(sender);
    }

    /// 控制台退了，之后的输出回到 stdout —— 否则关停阶段的日志会写进一个
    /// 没人再读的通道里。
    pub(crate) fn detach(&self) {
        *lock(&self.channel) = None;
    }

    fn emit(&self, message: &str) {
        let message = message.trim_end_matches('\n');
        if message.is_empty() {
            return;
        }

        match lock(&self.channel).as_ref() {
            // 通道是有界的。宁可丢弃也不能阻塞：调用方可能是业务线程，卡在
            // 这里等于让终端拖垮业务。
            Some(sender) => {
                let _ = sender.try_send(message.to_owned());
            }
            None => {
                let mut stdout = io::stdout().lock();
                let _ = writeln!(stdout, "{message}");
                let _ = stdout.flush();
            }
        }
    }

    /// 把暂存的内容交出去。
    fn take_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let message = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        self.emit(&message);
    }
}

impl Default for Printer {
    fn default() -> Self {
        Self {
            channel: Arc::default(),
            pending: Vec::new(),
        }
    }
}

impl Clone for Printer {
    /// 共用同一个通道，但各攒各的半行内容。
    fn clone(&self) -> Self {
        Self {
            channel: Arc::clone(&self.channel),
            pending: Vec::new(),
        }
    }
}

/// 给日志框架用的那一路：写多次，攒成一条。
impl Write for Printer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.take_pending();
        Ok(())
    }
}

impl Drop for Printer {
    /// 日志框架每条事件新拿一个 writer、写完就丢，收尾在这里。
    fn drop(&mut self) {
        self.take_pending();
    }
}

impl std::fmt::Debug for Printer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Printer")
            .field("attached", &lock(&self.channel).is_some())
            .field("pending", &self.pending.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam::channel::Receiver;

    fn attached(capacity: usize) -> (Printer, Receiver<String>) {
        let printer = Printer::new();
        let (sender, receiver) = crossbeam::channel::bounded(capacity);
        printer.attach(sender);
        (printer, receiver)
    }

    #[test]
    fn an_attached_printer_sends_to_the_channel() {
        let (printer, receiver) = attached(4);
        printer.print("你好");

        assert_eq!(receiver.try_recv().as_deref(), Ok("你好"));
    }

    /// 控制台退出后必须停止往通道里写 —— 那头已经没人读了。
    #[test]
    fn detaching_stops_the_channel() {
        let (printer, receiver) = attached(4);
        printer.detach();
        printer.print("落 stdout");

        assert!(receiver.try_recv().is_err(), "detach 之后不该再进通道");
    }

    /// 通道满了要丢弃而不是阻塞 —— 写入方可能是业务线程。
    #[test]
    fn a_full_channel_drops_instead_of_blocking() {
        let (printer, receiver) = attached(1);
        printer.print("第一条");
        // 满了；这几条会被丢掉，但调用本身必须立刻返回。
        printer.print("第二条");
        printer.print("第三条");

        assert_eq!(receiver.try_recv().as_deref(), Ok("第一条"));
        assert!(receiver.try_recv().is_err());
    }

    /// 结尾换行不该变成一行空白。
    #[test]
    fn trailing_newlines_are_trimmed() {
        let (printer, receiver) = attached(4);
        printer.print("一行\n");

        assert_eq!(receiver.try_recv().as_deref(), Ok("一行"));
        assert!(receiver.try_recv().is_err());
    }

    /// 空内容不该打出一行空白。
    #[test]
    fn nothing_is_printed_for_nothing() {
        let (printer, receiver) = attached(4);
        printer.print("");
        printer.print("\n");

        assert!(receiver.try_recv().is_err());
    }

    /// 日志框架那一路：一条事件写好几次，攒成一条交出去。
    #[test]
    fn the_writer_collects_one_event_into_one_message() {
        let (printer, receiver) = attached(4);

        let mut writer = printer.clone();
        write!(writer, "[12:00:00] ").unwrap();
        write!(writer, "[main/INFO]: ").unwrap();
        writeln!(writer, "上游 503").unwrap();
        assert!(receiver.try_recv().is_err(), "没 flush 之前不该发出去");

        drop(writer);
        assert_eq!(
            receiver.try_recv().as_deref(),
            Ok("[12:00:00] [main/INFO]: 上游 503")
        );
    }

    /// flush 与 drop 等价，且 flush 过的内容不会再发一次。
    #[test]
    fn flushing_hands_it_over_once() {
        let (printer, receiver) = attached(4);

        let mut writer = printer.clone();
        write!(writer, "一条").unwrap();
        writer.flush().unwrap();
        drop(writer);

        assert_eq!(receiver.try_recv().as_deref(), Ok("一条"));
        assert!(receiver.try_recv().is_err(), "drop 不该把它再发一遍");
    }

    /// 克隆共用通道，但不复制半行内容 —— 复制了会凭空多出一份。
    #[test]
    fn cloning_shares_the_channel_but_not_the_half_line() {
        let (printer, receiver) = attached(4);

        let mut writer = printer.clone();
        write!(writer, "半行").unwrap();
        let other = writer.clone();
        drop(other);
        assert!(receiver.try_recv().is_err(), "克隆不该带着半行内容一起走");

        drop(writer);
        assert_eq!(receiver.try_recv().as_deref(), Ok("半行"));
    }

    /// 多线程写是常态 —— 后台任务、指令线程、日志线程各写各的。
    #[test]
    fn many_threads_may_write() {
        let (printer, receiver) = attached(64);

        std::thread::scope(|scope| {
            for i in 0..8 {
                let printer = printer.clone();
                scope.spawn(move || printer.print(format!("第 {i} 条")));
            }
        });

        assert_eq!(receiver.len(), 8);
    }
}
