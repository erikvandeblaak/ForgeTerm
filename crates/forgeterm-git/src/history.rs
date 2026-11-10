//! History: commit list, one commit's details, commits per day.

use std::path::Path;

use chrono::{Duration as Days, NaiveDate, TimeZone};

use crate::{Diff, MAX_DIFF_LINES, READ_TIMEOUT, git};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub hash: String,
    pub short: String,
    pub author: String,
    /// Unix time the commit was authored.
    pub time: i64,
    /// Relative date as git formats it.
    pub when: String,
    /// Branch and tag names pointing here (`HEAD -> main, tag: v1`).
    pub refs: String,
    pub subject: String,
}

/// The last `count` commits of HEAD; empty without commits.
pub fn history(root: &Path, count: usize) -> Result<Vec<HistoryEntry>, String> {
    let n = format!("-n{count}");
    let out = git(
        root,
        &[
            "log",
            &n,
            "--no-color",
            "--no-show-signature",
            "--decorate=short",
            "--format=%H%x1f%h%x1f%an%x1f%at%x1f%ar%x1f%D%x1f%s%x1e",
        ],
        READ_TIMEOUT,
    )?;
    if !out.success() {
        if out.stderr.contains("does not have any commits") {
            return Ok(Vec::new());
        }
        return Err(out.error_text());
    }
    Ok(parse_history(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse_history(text: &str) -> Vec<HistoryEntry> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim_start_matches(['\r', '\n']);
            let f: Vec<&str> = record.split('\u{1f}').collect();
            if f.len() < 7 || f[0].trim().is_empty() {
                return None;
            }
            Some(HistoryEntry {
                hash: f[0].trim().to_owned(),
                short: f[1].to_owned(),
                author: f[2].to_owned(),
                time: f[3].trim().parse().unwrap_or(0),
                when: f[4].to_owned(),
                refs: f[5].to_owned(),
                subject: f[6].trim_end().to_owned(),
            })
        })
        .collect()
}

/// `git show` of one commit: header, changed-files summary and patch.
pub fn show_commit(root: &Path, hash: &str) -> Result<Diff, String> {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("not a commit hash".into());
    }
    let out = git(
        root,
        &[
            "show",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-show-signature",
            "--format=fuller",
            "--stat",
            "--patch",
            hash,
            "--",
        ],
        READ_TIMEOUT,
    )?;
    out.check()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let truncated = lines.len() > MAX_DIFF_LINES;
    lines.truncate(MAX_DIFF_LINES);
    Ok(Diff { lines, truncated })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    /// Oldest first; the last entry is today.
    pub days: Vec<(NaiveDate, u32)>,
    pub total: u32,
}

/// Commits on HEAD per local calendar day for the last `days` days (today
/// included), from the authored times.
pub fn activity(root: &Path, days: u32) -> Result<Activity, String> {
    let now = chrono::Local::now();
    let today = now.date_naive();
    // One day of slack: the exact day boundaries are applied below.
    let since = format!("--since={}", now.timestamp() - i64::from(days + 1) * 86_400);
    let out = git(
        root,
        &[
            "log",
            &since,
            "-n100000",
            "--no-show-signature",
            "--format=%at",
        ],
        READ_TIMEOUT,
    )?;
    if !out.success() {
        if out.stderr.contains("does not have any commits") {
            return Ok(daily_counts(&[], today, days, |_| None));
        }
        return Err(out.error_text());
    }
    let times: Vec<i64> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    Ok(daily_counts(&times, today, days, |t| {
        chrono::Local
            .timestamp_opt(t, 0)
            .single()
            .map(|d| d.date_naive())
    }))
}

/// Buckets `times` into the `days` calendar days ending with `today`, using
/// `to_day` for the time zone. Times outside the window are ignored.
pub fn daily_counts(
    times: &[i64],
    today: NaiveDate,
    days: u32,
    to_day: impl Fn(i64) -> Option<NaiveDate>,
) -> Activity {
    let days = days.max(1);
    let first = today - Days::days(i64::from(days) - 1);
    let mut buckets: Vec<(NaiveDate, u32)> = (0..days)
        .map(|i| (first + Days::days(i64::from(i)), 0))
        .collect();
    let mut total = 0;
    for &t in times {
        if let Some(day) = to_day(t)
            && day >= first
            && day <= today
        {
            let index = (day - first).num_days() as usize;
            buckets[index].1 += 1;
            total += 1;
        }
    }
    Activity {
        days: buckets,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn parses_history_records() {
        let text = "aaaa\u{1f}aa\u{1f}Erik\u{1f}1700000000\u{1f}2 days ago\u{1f}HEAD -> main, tag: v1\u{1f}Release\u{1e}\n\
                    bbbb\u{1f}bb\u{1f}Bot\u{1f}1690000000\u{1f}4 months ago\u{1f}\u{1f}Init\u{1e}\n";
        let h = parse_history(text);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].refs, "HEAD -> main, tag: v1");
        assert_eq!(h[0].time, 1_700_000_000);
        assert_eq!(h[1].refs, "");
        assert_eq!(h[1].subject, "Init");
    }

    #[test]
    fn counts_commits_per_day_in_the_window() {
        let utc = |t: i64| Utc.timestamp_opt(t, 0).single().map(|d| d.date_naive());
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        // 2023-11-14 22:13:20 UTC = 1700000000.
        let t = 1_700_000_000;
        let times = [t, t + 60, t - 86_400, t - 10 * 86_400, t + 86_400 * 5];
        let a = daily_counts(&times, day(2023, 11, 14), 7, utc);
        assert_eq!(a.days.len(), 7);
        assert_eq!(a.days[0].0, day(2023, 11, 8));
        assert_eq!(a.days[6], (day(2023, 11, 14), 2));
        assert_eq!(a.days[5], (day(2023, 11, 13), 1));
        // 10 days ago and in the future are outside the window.
        assert_eq!(a.total, 3);
        // Zero days is treated as one (today).
        assert_eq!(daily_counts(&[], day(2023, 1, 1), 0, utc).days.len(), 1);
    }
}
