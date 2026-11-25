use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use gpui_kit::{Hsla, Rgba, rgb};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Activity {
    pub id: usize,
    pub title: String,
    #[serde(rename = "start")]
    pub date: NaiveDate,
    // Keep the old field on disk until this activity is rescheduled.
    #[serde(rename = "end")]
    pub legacy_end: NaiveDate,
    pub time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub color: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Today,
    Upcoming,
    Distant,
    Past,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Today, Self::Upcoming, Self::Distant, Self::Past];
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Upcoming => "Upcoming",
            Self::Distant => "Distant",
            Self::Past => "Past",
        }
    }
}

impl Activity {
    pub fn group(&self, today: NaiveDate) -> Group {
        if self.date < today {
            Group::Past
        } else if self.date == today {
            Group::Today
        } else if (self.date - today).num_days() <= 7 {
            Group::Upcoming
        } else {
            Group::Distant
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("Enter an activity title.".into());
        }
        if self.title.chars().count() > 200 {
            return Err("Keep the title to 200 characters or fewer.".into());
        }
        if self.legacy_end < self.date {
            return Err("The end date must be on or after the start date.".into());
        }
        if self.time.is_none() && self.end_time.is_some() {
            return Err("Choose a start time first.".into());
        }
        Ok(())
    }
    pub fn reschedule(&mut self, date: NaiveDate) {
        if self.date != date {
            self.date = date;
            self.legacy_end = date;
        }
    }
    pub fn duration(&self) -> Option<TimeDelta> {
        let start = self.time?;
        let end = self.end_time?;
        let duration = end - start;
        Some(if duration <= TimeDelta::zero() {
            duration + TimeDelta::days(1)
        } else {
            duration
        })
    }
    pub fn schedule_at(&mut self, date: NaiveDate, start: NaiveTime) {
        self.end_time = self.duration().map(|duration| start + duration);
        self.time = Some(start);
        self.reschedule(date);
    }
    pub fn date_label(&self) -> String {
        self.date.format("%-d/%-m/%Y").to_string()
    }
    pub fn time_label(&self) -> Option<String> {
        self.time.map(|start| match self.end_time {
            Some(end) => time_range_label(start, end),
            None => start.format("%H:%M").to_string(),
        })
    }
    pub fn relative_label(&self, now: NaiveDateTime) -> String {
        if let Some(time) = self.time {
            let minutes = (self.date.and_time(time) - now).num_minutes();
            if minutes.abs() < 1 {
                return "Now".into();
            }
            let amount = if minutes.abs() < 60 {
                format!("{} min", minutes.abs())
            } else if minutes.abs() < 1440 {
                format!("{} hr", minutes.abs() / 60)
            } else {
                format!("{} days", minutes.abs() / 1440)
            };
            if minutes > 0 {
                format!("In {amount}")
            } else {
                format!("{amount} ago")
            }
        } else if self.date == now.date() {
            "Today".into()
        } else {
            self.date_label()
        }
    }
}

pub fn generate_color(
    activities: &[Activity],
    previous: Option<u32>,
    visible: RangeInclusive<NaiveDate>,
    scheduled: NaiveDate,
    mut rng: impl rand::Rng,
) -> Option<u32> {
    let used: HashSet<_> = activities
        .iter()
        .filter(|a| visible.contains(&a.date) || a.date == scheduled)
        .map(|a| a.color & 0xffffff)
        .collect();
    let previous = previous
        .map(|color| Hsla::from(rgb(color)))
        .filter(|color| color.s >= 0.15);

    const HUES: usize = 360;
    const SATURATIONS: usize = 16;
    const LIGHTNESSES: usize = 16;
    const COUNT: usize = HUES * SATURATIONS * LIGHTNESSES;
    let start = rng.random_range(0..COUNT);
    (0..COUNT).find_map(|offset| {
        let index = (start + offset) % COUNT;
        let h = (index % HUES) as f32 / HUES as f32;
        // Reserve a 76-degree gap on both sides of the previous hue.
        let h = previous.map_or(h, |color| (color.h + (76.0 + h * 208.0) / 360.0).fract());
        let s = 0.66 + (index / HUES % SATURATIONS) as f32 * 0.018;
        let l = 0.40 + (index / (HUES * SATURATIONS)) as f32 * 0.013;
        let color = u32::from(Rgba::from(Hsla { h, s, l, a: 1.0 })) >> 8;
        (!used.contains(&color)).then_some(color)
    })
}

pub fn end_time_label(start: NaiveTime, end: NaiveTime) -> String {
    let next_day = if end <= start { " (+1 day)" } else { "" };
    format!("{}{next_day}", end.format("%H:%M"))
}

fn time_range_label(start: NaiveTime, end: NaiveTime) -> String {
    format!("{} → {}", start.format("%H:%M"), end_time_label(start, end))
}

pub fn parse_time(value: &str) -> Result<NaiveTime, String> {
    let value = value.trim();
    let time_part = |part: &str| {
        !part.is_empty() && part.len() <= 2 && part.bytes().all(|b| b.is_ascii_digit())
    };
    let (hours, minutes) = if let Some(parts) = value.split_once(':') {
        parts
    } else if value.len() <= 2 {
        (value, "00")
    } else if value.len() <= 4 && value.bytes().all(|b| b.is_ascii_digit()) {
        value.split_at(value.len() - 2)
    } else {
        return Err("Use 24-hour time, for example 09:30.".into());
    };
    if !time_part(hours) || !time_part(minutes) {
        return Err("Use 24-hour time, for example 09:30.".into());
    }
    let total_minutes = hours.parse::<u32>().unwrap() * 60 + minutes.parse::<u32>().unwrap();
    let seconds = (total_minutes % (24 * 60)) * 60;
    Ok(NaiveTime::from_num_seconds_from_midnight_opt(seconds, 0).unwrap())
}

pub fn default_storage_path(root: &Path) -> PathBuf {
    let current = root.join("qalendar").join("activities.json");
    let previous = root.join("Curated").join("activities.json");
    // Keep using the existing store and lock when upgrading from the previous name.
    if matches!(current.try_exists(), Ok(false)) && !matches!(previous.try_exists(), Ok(false)) {
        previous
    } else {
        current
    }
}

pub fn load(path: &Path) -> Result<Vec<Activity>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("Could not read activities: {e}")),
    };
    let activities: Vec<Activity> =
        serde_json::from_slice(&bytes).map_err(|e| format!("Could not read activities: {e}"))?;
    let mut ids = HashSet::new();
    for a in &activities {
        a.validate()?;
        if !ids.insert(a.id) {
            return Err("The activity file contains duplicate IDs.".into());
        }
    }
    Ok(activities)
}

pub fn save(path: &Path, previous: &[Activity], activities: &[Activity]) -> Result<(), String> {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.try_lock()?;
        if load(path)? != previous {
            return Err(
                "Activities changed in another window. Reopen the app to load those changes".into(),
            );
        }
        let bytes = serde_json::to_vec_pretty(activities)?;
        let temporary = path.with_extension("json.tmp");
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, path)?;
        Ok(())
    };
    write()
        .map_err(|e| format!("Could not save activities: {e}. Your changes have not been applied."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn example(id: usize, offset: i64) -> Activity {
        let date = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap() + chrono::Duration::days(offset);
        Activity {
            id,
            title: "Visit".into(),
            date,
            legacy_end: date,
            time: None,
            end_time: None,
            color: 0xa52cff,
        }
    }
    #[test]
    fn activities_group_relative_to_today() {
        let today = example(1, 0).date;
        let items = [
            example(1, 0),
            example(2, 1),
            example(3, 7),
            example(4, 8),
            example(5, -1),
        ];
        assert_eq!(
            items.iter().map(|a| a.group(today)).collect::<Vec<_>>(),
            vec![
                Group::Today,
                Group::Upcoming,
                Group::Upcoming,
                Group::Distant,
                Group::Past
            ]
        );
        let mut ongoing = example(6, -2);
        ongoing.legacy_end = today;
        assert_eq!(ongoing.group(today), Group::Past);
    }

    #[test]
    fn rescheduling_a_legacy_activity_replaces_its_date_and_preserves_its_times() {
        let mut activity = example(1, 0);
        let target = activity.date + chrono::Days::new(1);
        activity.legacy_end = target + chrono::Days::new(2);
        activity.time = Some(parse_time("23:30").unwrap());
        activity.end_time = Some(parse_time("01:00").unwrap());
        let mut moved = activity.clone();
        moved.reschedule(target);
        assert_eq!(
            moved,
            Activity {
                date: target,
                legacy_end: target,
                ..activity
            }
        );
        assert!(moved.validate().is_ok());
        assert_eq!(
            moved.time_label().as_deref(),
            Some("23:30 → 01:00 (+1 day)")
        );
    }

    #[test]
    fn scheduling_at_a_new_time_preserves_duration_and_optional_end() {
        let mut activity = example(1, 0);
        let target = activity.date + chrono::Days::new(1);
        activity.schedule_at(target, parse_time("23:30").unwrap());
        assert_eq!(activity.date, target);
        assert_eq!(activity.end_time, None);

        activity.end_time = Some(parse_time("01:00").unwrap());
        activity.schedule_at(target, parse_time("09:00").unwrap());
        assert_eq!(activity.time_label().as_deref(), Some("09:00 → 10:30"));
    }

    #[test]
    fn timeline_stacks_overlaps_and_carries_overnight_blocks_into_the_next_day() {
        let date = example(1, 0).date;
        let scheduled = |id, offset, start, end| {
            let mut activity = example(id, offset);
            activity.time = Some(parse_time(start).unwrap());
            activity.end_time = Some(parse_time(end).unwrap());
            activity
        };
        let activities = [
            scheduled(1, -1, "23:00", "01:00"),
            scheduled(2, 0, "00:30", "01:30"),
            scheduled(3, 0, "01:00", "02:00"),
            example(4, 0),
        ];
        let blocks = crate::calendar::time_blocks(&activities, date, 60.0);
        let positions: Vec<_> = blocks.iter().map(|b| (b.left, b.right, b.lane)).collect();
        assert_eq!(
            positions,
            [(0.0, 60.0, 0), (30.0, 90.0, 1), (60.0, 120.0, 0)]
        );
    }

    #[test]
    fn generated_color_avoids_the_previous_hue_and_conflicts_in_either_range() {
        let today = example(1, 0).date;
        let visible = (today - chrono::Days::new(7))..=(today + chrono::Days::new(7));
        let scheduled = today + chrono::Days::new(40);
        let blue = 0x2864ee;
        let generate = |activities: &[Activity]| {
            generate_color(
                activities,
                Some(blue),
                visible.clone(),
                scheduled,
                StdRng::seed_from_u64(42),
            )
            .unwrap()
        };

        let first = generate(&[]);
        let hue_distance = (Hsla::from(rgb(first)).h - Hsla::from(rgb(blue)).h).abs();
        assert!(hue_distance.min(1.0 - hue_distance) >= 75.0 / 360.0);

        let mut same_color = example(2, 8);
        same_color.color = first;
        assert_eq!(generate(&[same_color.clone()]), first);
        same_color.date = *visible.end();
        assert_ne!(generate(&[same_color.clone()]), first);
        same_color.date = scheduled;
        assert_ne!(generate(&[same_color]), first);
    }
    #[test]
    fn validates_times_and_overnight_ranges() {
        for invalid in ["12:3x", "💥:", "9:", "93000"] {
            assert!(parse_time(invalid).is_err());
        }
        assert!(parse_time("00:00").is_ok());
        assert!(parse_time("23:59").is_ok());
        let mut a = example(1, 0);
        a.time = Some(parse_time("23:30").unwrap());
        a.end_time = Some(parse_time("01:00").unwrap());
        assert!(a.validate().is_ok());
        a.time = None;
        assert!(a.validate().is_err());
        a.end_time = None;
        a.title = " ".into();
        assert!(a.validate().is_err());
    }
    #[test]
    fn new_storage_prefers_the_existing_file_until_the_new_path_is_populated() {
        let root = std::env::temp_dir().join(format!("qalendar-path-test-{}", std::process::id()));
        let current = root.join("qalendar").join("activities.json");
        let previous = root.join("Curated").join("activities.json");
        assert_eq!(default_storage_path(&root), current);

        fs::create_dir_all(previous.parent().unwrap()).unwrap();
        fs::write(&previous, "[]").unwrap();
        assert_eq!(default_storage_path(&root), previous);

        fs::create_dir_all(current.parent().unwrap()).unwrap();
        fs::write(&current, "[]").unwrap();
        assert_eq!(default_storage_path(&root), current);

        for path in [current, previous] {
            fs::remove_file(&path).unwrap();
            fs::remove_dir(path.parent().unwrap()).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn storage_round_trip_and_replacement() {
        let path = std::env::temp_dir().join(format!("qalendar-test-{}.json", std::process::id()));
        let items = vec![example(1, 0), example(2, 5)];
        save(&path, &[], &items).unwrap();
        assert_eq!(load(&path).unwrap(), items);
        save(&path, &items, &items[..1]).unwrap();
        assert_eq!(load(&path).unwrap(), items[..1]);
        assert!(save(&path, &items, &items).is_err());
        assert_eq!(load(&path).unwrap(), items[..1]);
        fs::write(&path, "broken").unwrap();
        assert!(load(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::remove_file(path.with_extension("lock")).unwrap();
    }
}
