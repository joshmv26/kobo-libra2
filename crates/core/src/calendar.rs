//! Minimal iCalendar (.ics) reader for the home screen agenda.
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday, Utc};

#[derive(Debug, Clone)]
pub struct Entry {
    pub start: NaiveDateTime,
    pub all_day: bool,
    pub title: String,
}

struct Raw {
    start: NaiveDateTime,
    all_day: bool,
    title: String,
    freq: Option<String>,
    interval: i64,
    byday: Vec<Weekday>,
    until: Option<NaiveDate>,
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

fn parse_dt(params: &str, value: &str) -> Option<(NaiveDateTime, bool)> {
    let v = value.trim();
    if params.contains("VALUE=DATE") && !params.contains("DATE-TIME") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(&v[..8], "%Y%m%d").ok()?;
        return Some((d.and_time(NaiveTime::MIN), true));
    }
    let utc = v.ends_with('Z');
    let t = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    if utc {
        let local = Utc.from_utc_datetime(&t).with_timezone(&Local).naive_local();
        Some((local, false))
    } else {
        Some((t, false))
    }
}

fn weekday(code: &str) -> Option<Weekday> {
    match code.trim_start_matches(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
        "MO" => Some(Weekday::Mon), "TU" => Some(Weekday::Tue), "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu), "FR" => Some(Weekday::Fri), "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun), _ => None,
    }
}

fn parse(text: &str) -> Vec<Raw> {
    let mut out = Vec::new();
    let mut cur: Option<Raw> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            cur = Some(Raw { start: NaiveDateTime::MIN, all_day: false, title: String::new(), freq: None, interval: 1, byday: Vec::new(), until: None });
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(r) = cur.take() { if r.start != NaiveDateTime::MIN { out.push(r); } }
            continue;
        }
        let Some(r) = cur.as_mut() else { continue };
        let Some((name_params, value)) = line.split_once(':') else { continue };
        let (name, params) = name_params.split_once(';').unwrap_or((name_params, ""));
        match name {
            "DTSTART" => if let Some((d, a)) = parse_dt(params, value) { r.start = d; r.all_day = a; },
            "SUMMARY" => r.title = value.replace("\\,", ",").replace("\\n", " ").replace("\;", ";"),
            "RRULE" => {
                for part in value.split(';') {
                    if let Some((k, v)) = part.split_once('=') {
                        match k {
                            "FREQ" => r.freq = Some(v.to_string()),
                            "INTERVAL" => r.interval = v.parse().unwrap_or(1).max(1),
                            "BYDAY" => r.byday = v.split(',').filter_map(weekday).collect(),
                            "UNTIL" => r.until = NaiveDate::parse_from_str(&v[..v.len().min(8)], "%Y%m%d").ok(),
                            _ => {}
                        }
                    }
                }
            },
            _ => {}
        }
    }
    out
}

fn occurs(r: &Raw, d: NaiveDate) -> bool {
    let base = r.start.date();
    if d < base { return false; }
    let Some(f) = r.freq.as_deref() else { return d == base };
    if let Some(u) = r.until { if d > u { return false; } }
    let days = (d - base).num_days();
    match f {
        "DAILY" => days % r.interval == 0,
        "WEEKLY" => {
            let wk = (d.signed_duration_since(base).num_days() + base.weekday().num_days_from_monday() as i64) / 7;
            let ok_w = wk % r.interval == 0;
            if r.byday.is_empty() { ok_w && d.weekday() == base.weekday() } else { ok_w && r.byday.contains(&d.weekday()) }
        },
        "MONTHLY" => {
            let m = (d.year() - base.year()) as i64 * 12 + d.month() as i64 - base.month() as i64;
            m % r.interval == 0 && d.day() == base.day()
        },
        "YEARLY" => d.month() == base.month() && d.day() == base.day() && (d.year() - base.year()) as i64 % r.interval == 0,
        _ => false,
    }
}

/// Entries from `from` (inclusive) over the next `days` days, sorted by time.
pub fn upcoming(ics: &str, from: NaiveDate, days: i64) -> Vec<Entry> {
    let raws = parse(ics);
    let mut out = Vec::new();
    for i in 0..days {
        let d = from + Duration::days(i);
        for r in &raws {
            if occurs(r, d) {
                out.push(Entry { start: d.and_time(r.start.time()), all_day: r.all_day, title: r.title.clone() });
            }
        }
    }
    out.sort_by_key(|e| (e.start.date(), !e.all_day, e.start.time()));
    out
}

pub fn today() -> NaiveDate { Local::now().date_naive() }
