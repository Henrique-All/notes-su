use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, Utc};

use crate::models::{RecurringMode, Reminder, ReminderMode};

pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn next_occurrence(reminder: &Reminder, after_timestamp: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if reminder.disabled || reminder.deleted {
        return None;
    }

    let scheduled = match reminder.mode {
        ReminderMode::Permanent => Some(reminder.date.max(after_timestamp)),
        ReminderMode::Once => (reminder.date > after_timestamp).then_some(reminder.date),
        ReminderMode::Repeat => compute_repeat_occurrence(reminder, after_timestamp),
    };

    if let Some(snooze) = reminder.snooze_until
        && snooze > after_timestamp
    {
        return Some(match scheduled {
            Some(sched) => snooze.min(sched),
            None => snooze,
        });
    }

    scheduled
}

fn compute_repeat_occurrence(
    reminder: &Reminder,
    after_timestamp: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let base_time = reminder.date.time();
    match reminder.recurring_mode.unwrap_or(RecurringMode::Day) {
        RecurringMode::Day => {
            if reminder.date > after_timestamp {
                return Some(reminder.date);
            }
            let cand = after_timestamp.date_naive().and_time(base_time).and_utc();
            if cand > after_timestamp {
                Some(cand)
            } else {
                Some(cand + Duration::days(1))
            }
        }
        RecurringMode::Week => {
            let mut weekdays: BTreeSet<u8> = reminder
                .selected_days
                .iter()
                .map(|&d| if d == 7 { 0 } else { d })
                .filter(|&d| d <= 6)
                .collect();
            if weekdays.is_empty() {
                weekdays.insert(reminder.date.weekday().num_days_from_sunday() as u8);
            }

            let start_date = after_timestamp
                .date_naive()
                .max(reminder.date.date_naive());
            for offset in 0..=8u64 {
                let d = start_date.checked_add_days(Days::new(offset))?;
                let dow = d.weekday().num_days_from_sunday() as u8;
                if weekdays.contains(&dow) {
                    let dt = d.and_time(base_time).and_utc();
                    if dt > after_timestamp && dt >= reminder.date {
                        return Some(dt);
                    }
                }
            }
            None
        }
        RecurringMode::Month => {
            let mut month_days: Vec<u8> = reminder
                .selected_days
                .iter()
                .copied()
                .filter(|&d| (1..=31).contains(&d))
                .collect();
            if month_days.is_empty() {
                month_days.push(reminder.date.day() as u8);
            }

            let anchor = after_timestamp.max(reminder.date);
            let mut year = anchor.year();
            let mut month = anchor.month();

            for _ in 0..24 {
                let max_day = days_in_month(year, month);
                let mut candidates: BTreeSet<DateTime<Utc>> = BTreeSet::new();
                for &d in &month_days {
                    let clamped = (d as u32).min(max_day);
                    if let Some(nd) = NaiveDate::from_ymd_opt(year, month, clamped) {
                        candidates.insert(nd.and_time(base_time).and_utc());
                    }
                }
                if let Some(found) = candidates
                    .into_iter()
                    .find(|&dt| dt > after_timestamp && dt >= reminder.date)
                {
                    return Some(found);
                }
                if month == 12 {
                    year += 1;
                    month = 1;
                } else {
                    month += 1;
                }
            }
            None
        }
        RecurringMode::Year => {
            let base_month = reminder.date.month();
            let base_day = reminder.date.day();
            let start_year = after_timestamp.year().max(reminder.date.year());

            for year in start_year..start_year + 8 {
                let clamped = base_day.min(days_in_month(year, base_month));
                if let Some(nd) = NaiveDate::from_ymd_opt(year, base_month, clamped) {
                    let dt = nd.and_time(base_time).and_utc();
                    if dt > after_timestamp && dt >= reminder.date {
                        return Some(dt);
                    }
                }
            }
            None
        }
    }
}
