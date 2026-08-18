//! 各处共用的零碎工具。

use std::sync::{Mutex, MutexGuard};

/// 取锁；锁中毒了就把内部值取回来接着用。
///
/// 这些锁护的都是输出通道、输入行影子之类的小状态。别处 panic 过一次不该让
/// 它们整体停摆 —— 何况数据本身仍是完整的，中毒只说明有人在持锁时 panic
/// 了。尤其是输出：控制台崩掉的那一刻，恰恰是最需要它还能写字的时候。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
