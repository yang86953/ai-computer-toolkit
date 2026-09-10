//! 提供与 provider 无关的稳定持续时间状态机和可取消轮询间隔。

// 导入单调时钟与持续时间。
use std::time::{Duration, Instant};

// 表示一次有界查询的封闭匹配结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitObservation<'identity> {
    // 当前采样没有匹配元素。
    Missing,
    // 当前采样存在多个匹配元素。
    Ambiguous,
    // 当前采样唯一命中一个元素。
    Unique {
        // 借用当前 opaque element ID。
        element_id: &'identity str,
        // 标记调用方状态条件是否满足。
        condition_met: bool,
    },
}

// 表示稳定状态机对本次采样的判定。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitDecision {
    // 尚未达到稳定持续时间，应继续采样。
    Pending,
    // 唯一元素已经连续满足条件。
    Satisfied {
        // 返回实际连续稳定毫秒数。
        stable_for_ms: u64,
    },
    // 多命中必须立即 fail closed。
    Ambiguous,
}

// 跟踪同一 opaque element 连续满足条件的时间。
pub(crate) struct StableMatchTracker {
    // 保存契约要求的稳定持续时间。
    required_stable_ms: u64,
    // 保存当前连续满足条件的 element ID。
    current_element_id: Option<String>,
    // 保存当前连续区间的单调起始毫秒。
    satisfied_since_ms: Option<u64>,
}

// 跟踪一个布尔条件连续成立的时间。
pub(crate) struct StableConditionTracker {
    // 保存契约要求的连续稳定毫秒数。
    required_stable_ms: u64,
    // 保存当前连续成立区间的单调起始毫秒。
    satisfied_since_ms: Option<u64>,
}

// 实现与具体 provider 无关的布尔稳定状态机。
impl StableConditionTracker {
    // 创建尚未观察到条件成立的新状态机。
    pub(crate) fn new(required_stable_ms: u64) -> Self {
        // 返回只拥有稳定时间配置的初始状态。
        Self {
            // 保存调用方要求的稳定时间。
            required_stable_ms,
            // 初始不存在连续成立区间。
            satisfied_since_ms: None,
        }
    }

    // 接收一次布尔采样并推进连续稳定状态。
    pub(crate) fn observe(&mut self, now_ms: u64, condition_met: bool) -> WaitDecision {
        // 条件不成立时立即清空旧连续区间。
        if !condition_met {
            // 移除旧起点，后续成立必须重新计时。
            self.satisfied_since_ms = None;
            // 尚未满足稳定条件。
            return WaitDecision::Pending;
        }
        // 首次成立时保存当前单调时刻。
        let satisfied_since_ms = *self.satisfied_since_ms.get_or_insert(now_ms);
        // 计算当前连续成立时长。
        let stable_for_ms = now_ms.saturating_sub(satisfied_since_ms);
        // 达到调用方要求后返回成功判定。
        if stable_for_ms >= self.required_stable_ms {
            // 返回实际连续稳定毫秒数。
            return WaitDecision::Satisfied { stable_for_ms };
        }
        // 尚未达到稳定时间时继续采样。
        WaitDecision::Pending
    }
}

// 实现确定性稳定持续时间状态机。
impl StableMatchTracker {
    // 创建空状态机。
    pub(crate) fn new(required_stable_ms: u64) -> Self {
        // 返回不包含旧采样事实的新状态。
        Self {
            // 保存稳定持续时间要求。
            required_stable_ms,
            // 初始没有匹配元素。
            current_element_id: None,
            // 初始没有连续区间。
            satisfied_since_ms: None,
        }
    }

    // 接收一次采样并推进状态。
    pub(crate) fn observe(
        // 借用可变状态机。
        &mut self,
        // 接收从操作起点计算的单调毫秒数。
        now_ms: u64,
        // 接收封闭匹配结果。
        observation: WaitObservation<'_>,
    ) -> WaitDecision {
        // 多命中永不等待到歧义自行消失。
        if observation == WaitObservation::Ambiguous {
            // 清空连续状态。
            self.reset();
            // 返回 fail-closed 判定。
            return WaitDecision::Ambiguous;
        }
        // 缺失或状态不满足会中断连续区间。
        let WaitObservation::Unique {
            // 读取唯一 element ID。
            element_id,
            // 读取状态满足标记。
            condition_met: true,
        } = observation
        else {
            // 清空旧元素与起点。
            self.reset();
            // 继续等待后续采样。
            return WaitDecision::Pending;
        };
        // 元素身份变化必须重新开始稳定计时。
        if self.current_element_id.as_deref() != Some(element_id) {
            // 保存新 opaque ID。
            self.current_element_id = Some(element_id.to_owned());
            // 从当前采样时刻开始连续区间。
            self.satisfied_since_ms = Some(now_ms);
        }
        // 已设置起点时计算连续时长。
        let stable_for_ms = self
            // 读取确定存在的起点。
            .satisfied_since_ms
            // 使用饱和减法抵御错误的非单调调用。
            .map(|since| now_ms.saturating_sub(since))
            // 防御性使用零值。
            .unwrap_or(0);
        // 达到或超过要求时成功。
        if stable_for_ms >= self.required_stable_ms {
            // 返回实际稳定时长证据。
            return WaitDecision::Satisfied { stable_for_ms };
        }
        // 尚未达到要求时继续。
        WaitDecision::Pending
    }

    // 清空连续满足状态。
    fn reset(&mut self) {
        // 移除旧 element ID。
        self.current_element_id = None;
        // 移除旧连续起点。
        self.satisfied_since_ms = None;
    }
}

// 等待一个有界轮询间隔，并以短切片响应取消。
pub(crate) fn cancellable_pause(
    // 接收总暂停时间。
    duration: Duration,
    // 接收无副作用取消探针。
    mut is_cancelled: impl FnMut() -> bool,
) -> bool {
    // 记录单调起点。
    let started = Instant::now();
    // 循环到时间耗尽或取消。
    loop {
        // 取消时立即返回未完成。
        if is_cancelled() {
            // 向调用方报告取消。
            return false;
        }
        // 计算已经等待的时间。
        let elapsed = started.elapsed();
        // 达到总时间时报告完成。
        if elapsed >= duration {
            // 返回正常完成。
            return true;
        }
        // 计算剩余间隔。
        let remaining = duration.saturating_sub(elapsed);
        // 每 10ms 至少检查一次取消。
        let slice = remaining.min(Duration::from_millis(10));
        // 只暂停当前短切片。
        std::thread::sleep(slice);
    }
}

// 验证稳定时间、身份变化和取消语义。
#[cfg(test)]
mod tests {
    // 导入待测状态机与暂停函数。
    use super::*;

    // 验证只有同一元素连续满足条件才能成功。
    #[test]
    fn stable_tracker_resets_on_missing_state_or_identity_change() {
        // 要求连续 300ms。
        let mut tracker = StableMatchTracker::new(300);
        // 首次满足只建立起点。
        assert_eq!(
            tracker.observe(
                100,
                WaitObservation::Unique {
                    element_id: "s2:e:first",
                    condition_met: true,
                }
            ),
            WaitDecision::Pending
        );
        // 未达到持续时间时继续。
        assert_eq!(
            tracker.observe(
                399,
                WaitObservation::Unique {
                    element_id: "s2:e:first",
                    condition_met: true,
                }
            ),
            WaitDecision::Pending
        );
        // 元素身份变化重置计时。
        assert_eq!(
            tracker.observe(
                400,
                WaitObservation::Unique {
                    element_id: "s2:e:second",
                    condition_met: true,
                }
            ),
            WaitDecision::Pending
        );
        // 同一新元素达到稳定时间后成功。
        assert_eq!(
            tracker.observe(
                700,
                WaitObservation::Unique {
                    element_id: "s2:e:second",
                    condition_met: true,
                }
            ),
            WaitDecision::Satisfied { stable_for_ms: 300 }
        );
        // 缺失会清空已成功区间。
        assert_eq!(
            tracker.observe(701, WaitObservation::Missing),
            WaitDecision::Pending
        );
    }

    // 验证多命中立即返回歧义。
    #[test]
    fn stable_tracker_fails_closed_on_ambiguity() {
        // 创建零稳定时间状态机。
        let mut tracker = StableMatchTracker::new(0);
        // 多命中不得进入成功状态。
        assert_eq!(
            tracker.observe(0, WaitObservation::Ambiguous),
            WaitDecision::Ambiguous
        );
    }

    // 验证布尔条件在中断后必须重新累计稳定时间。
    #[test]
    fn stable_condition_tracker_resets_after_condition_breaks() {
        // 要求连续一百毫秒。
        let mut tracker = StableConditionTracker::new(100);
        // 首次成立只建立起点。
        assert_eq!(tracker.observe(10, true), WaitDecision::Pending);
        // 条件中断会清空旧区间。
        assert_eq!(tracker.observe(60, false), WaitDecision::Pending);
        // 再次成立必须从新时刻开始。
        assert_eq!(tracker.observe(70, true), WaitDecision::Pending);
        // 新区间达到一百毫秒后才能成功。
        assert_eq!(
            tracker.observe(170, true),
            WaitDecision::Satisfied { stable_for_ms: 100 }
        );
    }

    // 验证暂停能够在不等待完整间隔时响应取消。
    #[test]
    fn pause_reports_cancellation() {
        // 立即取消必须返回 false。
        assert!(!cancellable_pause(Duration::from_secs(1), || true));
        // 零时长且未取消必须立即完成。
        assert!(cancellable_pause(Duration::ZERO, || false));
    }
}
