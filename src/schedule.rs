//! Numeric five-field cron with IANA timezones and traditional DOM/DOW semantics.
use anyhow::{bail, ensure, Result};
use chrono::{Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

#[derive(Debug)]
pub struct Cron {
    fields: [u64; 5],
    any_dom: bool,
    any_dow: bool,
}

impl Cron {
    pub fn parse(expression: &str) -> Result<Self> {
        ensure!(expression.len() <= 160, "Cron expression is too long");
        let expression = match expression {
            "hourly" => "0 * * * *",
            "daily" => "0 0 * * *",
            "weekly" => "0 0 * * 0",
            value => value,
        };
        let parts: Vec<_> = expression.split_whitespace().collect();
        ensure!(
            parts.len() == 5,
            "Use five cron fields: minute hour day month weekday"
        );
        let mut fields = [0u64; 5];
        for (index, (min, max)) in [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)]
            .into_iter()
            .enumerate()
        {
            for segment in parts[index].split(',') {
                let mut step_parts = segment.split('/');
                let base = step_parts.next().unwrap_or("");
                let step: u32 = step_parts
                    .next()
                    .unwrap_or("1")
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid cron step"))?;
                ensure!(
                    step > 0 && step <= 60 && step_parts.next().is_none(),
                    "Invalid cron step"
                );
                let (start, end) = if base == "*" {
                    (min, max)
                } else if let Some((a, b)) = base.split_once('-') {
                    (a.parse::<u32>()?, b.parse::<u32>()?)
                } else {
                    let value = base.parse::<u32>()?;
                    (value, value)
                };
                ensure!(
                    start >= min && end <= max && start <= end,
                    "Cron field is out of range"
                );
                for value in (start..=end).step_by(step as usize) {
                    fields[index] |= 1 << if index == 4 && value == 7 { 0 } else { value };
                }
            }
        }
        Ok(Self {
            fields,
            any_dom: parts[2].starts_with('*'),
            any_dow: parts[4].starts_with('*'),
        })
    }

    pub fn next(&self, timezone: &str, after: i64) -> Result<i64> {
        let zone: Tz = timezone
            .parse()
            .map_err(|_| anyhow::anyhow!("Unknown IANA timezone"))?;
        let first = after.div_euclid(60) * 60 + 60;
        // Five years includes the next leap day, even across a non-leap century.
        for offset in 0..(366 * 5 * 24 * 60) {
            let timestamp = first + offset * 60;
            let date = Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .ok_or_else(|| anyhow::anyhow!("Invalid schedule date"))?
                .with_timezone(&zone);
            let has = |index: usize, value: u32| self.fields[index] & (1 << value) != 0;
            if !has(0, date.minute()) || !has(1, date.hour()) || !has(3, date.month()) {
                continue;
            }
            let dom = has(2, date.day());
            let dow = has(4, date.weekday().num_days_from_sunday());
            if if self.any_dom || self.any_dow {
                dom && dow
            } else {
                dom || dow
            } {
                return Ok(timestamp);
            }
        }
        bail!("Cron expression has no occurrence within five years")
    }
}

pub fn next(expression: &str, timezone: &str, after: i64) -> Result<i64> {
    Cron::parse(expression)?.next(if timezone.is_empty() { "UTC" } else { timezone }, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ts(date: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(date)
            .unwrap()
            .timestamp()
    }
    #[test]
    fn numeric_ranges_steps_and_aliases() {
        assert_eq!(
            next("*/15 9-17 * * 1-5", "UTC", ts("2026-09-24T09:01:00Z")).unwrap(),
            ts("2026-09-24T09:15:00Z")
        );
        assert_eq!(
            next("weekly", "UTC", ts("2026-09-24T09:01:00Z")).unwrap(),
            ts("2026-09-27T00:00:00Z")
        );
        for bad in [
            "* * * *",
            "60 * * * *",
            "*/0 * * * *",
            "* * * * 8",
            "1-0 * * * *",
            "* * * * *;id",
        ] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn timezone_and_dst() {
        assert_eq!(
            next("0 9 * * *", "Asia/Tehran", ts("2026-09-24T00:00:00Z")).unwrap(),
            ts("2026-09-24T05:30:00Z")
        );
        assert_eq!(
            next("30 2 * * *", "America/New_York", ts("2026-03-08T05:00:00Z")).unwrap(),
            ts("2026-03-09T06:30:00Z")
        );
        assert!(next("* * * * *", "invalid/zone", 0).is_err());
    }
    #[test]
    fn day_of_month_or_weekday() {
        assert_eq!(
            next("0 0 1 * 1", "UTC", ts("2026-09-24T00:00:00Z")).unwrap(),
            ts("2026-09-28T00:00:00Z")
        );
    }
}
