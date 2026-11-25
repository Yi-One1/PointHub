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

/// 把 epoch 秒转成本地时间的便捷函数。
pub fn local_from_epoch(secs: i64) -> Option<NaiveDateTime> {
    Local.timestamp_opt(secs, 0).single().map(|d| d.naive_local())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
