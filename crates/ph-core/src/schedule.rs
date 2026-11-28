use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

/// 单账号的签到时间规则。
///
/// 设计要点：
/// - 每个账号有独立的时分，默认取自平台刷新时刻；
/// - `jitter_minutes` 让同平台多账号不在同一秒打上游（上游有并发/风控）；
/// - 抖动是**按账号 id 确定性**算的，不是每次随机，否则「今天 00:10、明天 00:13」
///   会让用户觉得时间在漂。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSchedule {
    pub hour: u8,
    pub minute: u8,
    /// 0 表示不抖动。抖动范围是 [0, jitter_minutes]。
    #[serde(default = "default_jitter")]
    pub jitter_minutes: u8,
}

fn default_jitter() -> u8 {
    5
}

impl AccountSchedule {
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            hour,
            minute,
            jitter_minutes: default_jitter(),
        }
    }

    /// 由平台基准 + 同平台序号算出账号时间；跨零点自动回卷（23:58 + 5 分钟 → 00:03）。
    ///
    /// 两部分：`hour/minute` 是**错峰后的段起点**，`jitter_minutes` 是**段内可浮动的分钟数**，
    /// `slot_on` 每天在这段里再取一个偏移（见 `deterministic_offset`）。
    ///
    /// 段按**左闭右开**切：第 i 个账号的落点是 `[start, end)`，最后一段保留右端点
    /// 以铺满窗口。这样相邻账号即使在端点也不会撞到同一分钟——`slot_on` 的秒恒为 0，
    /// 同一分钟就是同一秒打上游。窗口比账号数还窄时段宽会落到 0，界面会提示用户放宽窗口。
    pub fn from_platform(
        base_hour: u8,
        base_minute: u8,
        index: usize,
        total: usize,
        spread_minutes: u8,
    ) -> Self {
        let (start, end) = cell_bounds(index, total, spread_minutes);
        let base = (base_hour as u32 % 24) * 60 + (base_minute as u32 % 60);
        // 按「一天的总分钟数」取模，23:58 + 5 分钟自然回卷到次日 00:03。
        let mins = (base + start as u32) % (24 * 60);
        let width = if index.min(total.saturating_sub(1)) + 1 >= total {
            // 最后一段：保留右端点，单账号时整段窗口都归它。
            end - start
        } else {
            // 前面的段：右端点留给下一段，所以少给 1 分钟。
            (end - start).saturating_sub(1)
        };
        Self {
            hour: (mins / 60) as u8,
            minute: (mins % 60) as u8,
            jitter_minutes: width,
        }
    }

    /// 算出某个日期该账号实际执行的本地时间。
    pub fn slot_on(&self, date: NaiveDate, account_id: &str) -> NaiveDateTime {
        let base_time = NaiveTime::from_hms_opt(self.hour as u32, self.minute as u32, 0)
            .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).expect("00:00 一定合法"));
        let offset = self.deterministic_offset(account_id, date);
        let naive = date.and_time(base_time);
        naive + Duration::minutes(offset as i64)
    }

    /// 稳定偏移：同一天同一账号结果恒定，不同账号错开。
    fn deterministic_offset(&self, account_id: &str, date: NaiveDate) -> u8 {
        if self.jitter_minutes == 0 {
            return 0;
        }
        // FNV-1a：短、无依赖、分布够用。
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in account_id.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for b in date.to_string().as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        // FNV-1a 的低位只由输入字节的奇偶决定：账号 id 字节奇偶相同时，
        // 两个账号天天取到同一个偏移（段宽为 1 时尤其明显）。最后过一次
        // murmur3 finalizer 把高位混下来，再取模才谈得上「随机」。
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51afd7ed558ccd);
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
        hash ^= hash >> 33;
        (hash % (self.jitter_minutes as u64 + 1)) as u8
    }

    /// 下一次应当执行的时间点（本地时间）。若今天的时间点已过，则返回明天。
    pub fn next_run_after(&self, now: NaiveDateTime, account_id: &str) -> NaiveDateTime {
        let today = now.date();
        let today_slot = self.slot_on(today, account_id);
        if today_slot > now {
            today_slot
        } else {
            self.slot_on(today + Duration::days(1), account_id)
        }
    }

    /// 今天是否已经过了执行时间点（用于「启动补签」判断）。
    pub fn due_today(&self, now: NaiveDateTime, account_id: &str) -> bool {
        self.slot_on(now.date(), account_id) <= now
    }

    pub fn describe(&self) -> String {
        if self.jitter_minutes == 0 {
            format!("每天 {:02}:{:02}", self.hour, self.minute)
        } else {
            format!(
                "每天 {:02}:{:02} + 0~{} 分钟随机",
                self.hour, self.minute, self.jitter_minutes
            )
        }
    }
}

/// 把「浮动窗口」切成 `total` 段，返回第 `index` 段在窗口内的 `[起, 止]` 分钟偏移。
///
/// 为什么分段而不是所有账号共用一个窗口：`total` 个账号都从基准时刻起随机，
/// 迟早会撞在同一分钟上（窗口越窄越容易），而错峰的目的正是不要撞。
/// 分段后每段互不重叠，同一账号又能在自己那一段里每天浮动（`slot_on` 用
/// `jitter_minutes` 做按日的确定性偏移）。
///
/// `total == 0` 或 `spread == 0` → `(0, 0)`（不浮动）；`index` 越界按最后一段截断，绝不 panic。
pub fn cell_bounds(index: usize, total: usize, spread_minutes: u8) -> (u8, u8) {
    if total == 0 || spread_minutes == 0 {
        return (0, 0);
    }
    let spread = spread_minutes as usize;
    let idx = index.min(total - 1);
    // round(k * spread / total)：整数除法会把末段挤成 0 宽，四舍五入让窗口铺满。
    let bound = |k: usize| -> u8 { ((k * spread * 2 + total) / (total * 2)) as u8 };
    (bound(idx), bound(idx + 1))
}

/// 把 epoch 秒转成本地时间的便捷函数。
pub fn local_from_epoch(secs: i64) -> Option<NaiveDateTime> {
    Local.timestamp_opt(secs, 0).single().map(|d| d.naive_local())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn slot_is_stable_within_a_day() {
        let s = AccountSchedule::new(0, 10);
        let d = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let a = s.slot_on(d, "acct-1");
        let b = s.slot_on(d, "acct-1");
        assert_eq!(a, b, "同一账号同一天必须稳定");
    }

    #[test]
    fn different_accounts_get_different_offsets() {
        let s = AccountSchedule::new(0, 10);
        let d = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let mut set = std::collections::HashSet::new();
        for i in 0..12 {
            set.insert(s.slot_on(d, &format!("acct-{i}")).time());
        }
        assert!(set.len() > 1, "多账号应当被错开");
    }

    #[test]
    fn next_run_rolls_to_tomorrow_after_slot() {
        let s = AccountSchedule::new(9, 0);
        let now = at(2026, 11, 25, 23, 0);
        let next = s.next_run_after(now, "acct");
        assert_eq!(next.date(), NaiveDate::from_ymd_opt(2026, 11, 26).unwrap());
    }

    #[test]
    fn due_today_respects_slot() {
        let s = AccountSchedule::new(9, 0);
        assert!(!s.due_today(at(2026, 11, 25, 8, 0), "acct"));
        assert!(s.due_today(at(2026, 11, 25, 10, 0), "acct"));
    }

    #[test]
    fn cell_bounds_splits_window_without_overlap() {
        // 3 个账号、5 分钟窗口：切成 [0,2] [2,3] [3,5]，首尾相接铺满窗口。
        assert_eq!(cell_bounds(0, 3, 5), (0, 2));
        assert_eq!(cell_bounds(1, 3, 5), (2, 3));
        assert_eq!(cell_bounds(2, 3, 5), (3, 5));
        // 单账号：整段窗口都归它，才有「每天浮动」的余地。
        assert_eq!(cell_bounds(0, 1, 5), (0, 5));
        // 窗口为 0 / 没有账号：不浮动。
        assert_eq!(cell_bounds(0, 3, 0), (0, 0));
        assert_eq!(cell_bounds(0, 0, 5), (0, 0));
        // 序号越界按最后一段截断，不 panic。
        assert_eq!(cell_bounds(9, 3, 5), (3, 5));
        assert_eq!(cell_bounds(usize::MAX, 3, 5), (3, 5));
    }

    #[test]
    fn cell_bounds_is_monotonic_and_covers_the_window() {
        // 任意账号数 × 任意窗口：段首尾相接、不重叠、不越界，末段止于 spread。
        for total in 1usize..=8 {
            for spread in 1u8..=30 {
                let mut prev_end = 0u8;
                for i in 0..total {
                    let (s, e) = cell_bounds(i, total, spread);
                    assert_eq!(s, prev_end, "第 {i} 段应与上一段首尾相接（total={total} spread={spread}）");
                    assert!(e >= s, "段宽不能为负（total={total} spread={spread} i={i}）");
                    prev_end = e;
                }
                assert_eq!(
                    prev_end, spread,
                    "最后一段必须铺到窗口末端（total={total} spread={spread}）"
                );
            }
        }
    }

    #[test]
    fn from_platform_rolls_over_midnight() {
        // 单账号：基准时刻不动，整段窗口作为每日浮动范围。
        let one = AccountSchedule::from_platform(23, 58, 0, 1, 5);
        assert_eq!((one.hour, one.minute, one.jitter_minutes), (23, 58, 5));
        // 3 个账号的最后一个：23:58 + 3 = 次日 00:01。
        let last = AccountSchedule::from_platform(23, 58, 2, 3, 5);
        assert_eq!((last.hour, last.minute, last.jitter_minutes), (0, 1, 2));
        // 23:59 起算，第二段 00:02，跨零点后 offset 仍然算在窗口里。
        let mid = AccountSchedule::from_platform(23, 59, 1, 3, 5);
        assert_eq!((mid.hour, mid.minute), (0, 1));
    }

    #[test]
    fn from_platform_floats_daily_inside_its_own_cell() {
        // 每个账号拿到的 jitter 就是自己那一段的宽度：不只是显示成
        // 「+0~N 分钟随机」好看，`slot_on` 真的会在这一段里按日取值。
        // 左闭右开：前两段各让出右端点，所以宽度是 1 / 0，末段保留右端点是 2。
        for (i, want) in [(0usize, 1u8), (1, 0), (2, 2)] {
            let s = AccountSchedule::from_platform(9, 30, i, 3, 5);
            assert_eq!(s.jitter_minutes, want, "第 {i} 段的宽度不对");
            let (start, _) = cell_bounds(i, 3, 5);
            // 1 月连续 10 天：落点必须落在 [基准+段首, 基准+段尾] 内。
            let mut seen = std::collections::BTreeSet::new();
            for d in 1..=10u32 {
                let date = NaiveDate::from_ymd_opt(2027, 1, d).unwrap();
                let slot = s.slot_on(date, &format!("acct-{i}"));
                let mins = (slot.hour() * 60 + slot.minute()) as i32 - (9 * 60 + 30);
                assert!(
                    mins >= start as i32 && mins <= (start as i32 + want as i32),
                    "第 {i} 段落点越界：{mins} 不在 [{start}, {}]",
                    start as i32 + want as i32
                );
                seen.insert(slot.time());
            }
            assert!(
                seen.len() > 1 || want == 0,
                "段宽 {want} 分钟时，10 天内应当出现不止一个落点"
            );
        }
    }
}
