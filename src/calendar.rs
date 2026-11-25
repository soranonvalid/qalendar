use crate::activity::Activity;
use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, Timelike};

pub fn shift_month(date: NaiveDate, delta: i32) -> NaiveDate {
    if delta < 0 {
        date.checked_sub_months(Months::new(delta.unsigned_abs()))
    } else {
        date.checked_add_months(Months::new(delta as u32))
    }
    .unwrap_or(date)
}

pub fn shift_day(date: NaiveDate, delta: i32) -> NaiveDate {
    if delta < 0 {
        date.checked_sub_days(Days::new(delta.unsigned_abs() as u64))
    } else {
        date.checked_add_days(Days::new(delta as u64))
    }
    .unwrap_or(date)
}

pub fn month_grid(date: NaiveDate) -> Vec<NaiveDate> {
    let first = date.with_day(1).unwrap();
    let offset = first.weekday().num_days_from_sunday();
    let days = (shift_month(first, 1) - first).num_days() as u32;
    let cells = (offset + days).div_ceil(7).max(5) * 7;
    let start = shift_day(first, -(offset as i32));
    (0..cells).map(|i| shift_day(start, i as i32)).collect()
}

#[derive(Debug)]
pub struct Timeline {
    pub zoom: i32,
    pub offset: f32,
}

pub struct TimeBlock<'a> {
    pub activity: &'a Activity,
    pub left: f32,
    pub right: f32,
    pub lane: usize,
}

pub fn time_blocks(
    activities: &[Activity],
    date: NaiveDate,
    hour_width: f32,
) -> Vec<TimeBlock<'_>> {
    let day_width = 24.0 * hour_width;
    let mut blocks: Vec<_> = activities
        .iter()
        .filter_map(|activity| {
            let start = activity.time?;
            let duration = activity.duration();
            let day_offset = if activity.date == date {
                0.0
            } else if activity.date.succ_opt() == Some(date) && duration.is_some() {
                -day_width
            } else {
                return None;
            };
            let start_x =
                day_offset + start.num_seconds_from_midnight() as f32 * hour_width / 3600.0;
            let end_x = duration.map_or(start_x + 8.0, |duration| {
                start_x + duration.as_seconds_f32() * hour_width / 3600.0
            });
            (end_x > 0.0).then_some(TimeBlock {
                activity,
                left: start_x.max(0.0),
                right: end_x.max(start_x + 2.0).min(day_width),
                lane: 0,
            })
        })
        .collect();
    blocks.sort_by(|a, b| {
        a.left
            .total_cmp(&b.left)
            .then_with(|| a.activity.id.cmp(&b.activity.id))
    });
    let mut lane_ends = Vec::new();
    for block in &mut blocks {
        if let Some(lane) = lane_ends.iter().position(|&end| end <= block.left) {
            lane_ends[lane] = block.right;
            block.lane = lane;
        } else {
            block.lane = lane_ends.len();
            lane_ends.push(block.right);
        }
    }
    blocks
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            zoom: 100,
            offset: 935.0,
        }
    }
}

impl Timeline {
    pub fn hour_width(&self) -> f32 {
        160.0 * self.zoom as f32 / 100.0
    }

    pub fn current_time_x(&self, selected: NaiveDate, now: NaiveDateTime) -> Option<f32> {
        (selected == now.date())
            .then(|| now.num_seconds_from_midnight() as f32 / 3600.0 * self.hour_width())
    }

    pub fn minute_at(x: f32, hour_width: f32) -> u32 {
        (x / hour_width * 60.0).floor().clamp(0.0, 1439.0) as u32
    }

    pub fn label_left(hour: u32, hour_width: f32, label_width: f32, offset: f32) -> f32 {
        let inset = if hour == 1 { label_width + 8.0 } else { 0.0 };
        (hour as f32 * hour_width - label_width / 2.0).max(inset) - offset
    }

    pub fn label_opacity(left: f32, label_width: f32, viewport_width: f32) -> f32 {
        let visible = (left + label_width).min(viewport_width) - left.max(0.0);
        let fraction = (visible / label_width).clamp(0.0, 1.0);
        fraction * fraction * (3.0 - 2.0 * fraction)
    }

    pub fn pan(&mut self, delta: f32, width: f32) {
        self.offset = (self.offset + delta).clamp(0.0, (24.0 * self.hour_width() - width).max(0.0));
    }

    pub fn zoom_at(&mut self, zoom: i32, anchor: f32, width: f32) {
        let hour = (self.offset + anchor) / self.hour_width();
        self.zoom = zoom.clamp(50, 200);
        self.offset = hour * self.hour_width() - anchor;
        self.pan(0.0, width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn cursor_time_uses_the_zoomed_quarter_hour_spacing() {
        assert_eq!(Timeline::minute_at(160.0 * 7.25, 160.0), 7 * 60 + 15);
        assert_eq!(Timeline::minute_at(120.0 * 7.75, 120.0), 7 * 60 + 45);
        assert_eq!(Timeline::minute_at(24.0 * 160.0, 160.0), 23 * 60 + 59);
    }

    #[test]
    fn current_time_marker_only_appears_on_today() {
        let today = date(2026, 11, 13);
        let now = today.and_hms_opt(7, 30, 0).unwrap();
        let timeline = Timeline::default();
        assert_eq!(timeline.current_time_x(today, now), Some(1200.0));
        assert_eq!(timeline.current_time_x(shift_day(today, -1), now), None);
    }

    #[test]
    fn labels_fade_symmetrically_at_the_viewport_edges() {
        assert_eq!(Timeline::label_opacity(0.0, 100.0, 600.0), 1.0);
        assert_eq!(Timeline::label_opacity(-50.0, 100.0, 600.0), 0.5);
        assert_eq!(Timeline::label_opacity(550.0, 100.0, 600.0), 0.5);
        assert_eq!(Timeline::label_opacity(-100.0, 100.0, 600.0), 0.0);
        assert_eq!(Timeline::label_opacity(600.0, 100.0, 600.0), 0.0);
    }

    #[test]
    fn midnight_and_consecutive_hour_labels_fit_at_low_zoom() {
        for zoom in [50, 75] {
            let timeline = Timeline { zoom, offset: 0.0 };
            let hour_width = timeline.hour_width();
            let label_width = 116.0 * zoom as f32 / 100.0;
            assert_eq!(Timeline::label_left(0, hour_width, label_width, 0.0), 0.0);
            let first_hour = Timeline::label_left(1, hour_width, label_width, 0.0);
            let second_hour = Timeline::label_left(2, hour_width, label_width, 0.0);
            assert!(first_hour >= label_width + 8.0);
            assert!(second_hour > first_hour + label_width);
        }
    }

    #[test]
    fn calendar_handles_real_weekdays_and_month_boundaries() {
        let october_fourth = date(2025, 10, 4);
        let october = month_grid(october_fourth);
        assert_eq!(october.len(), 35);
        assert_eq!(october[0], date(2025, 9, 28));
        assert_eq!(october[6], october_fourth);
        assert_eq!(october[34], date(2025, 11, 1));
        assert_eq!(month_grid(date(2025, 8, 1)).len(), 42);
        assert_eq!(shift_month(date(2024, 1, 31), 1), date(2024, 2, 29));
        assert_eq!(shift_month(date(2025, 1, 31), 1), date(2025, 2, 28));
        assert_eq!(shift_month(date(2025, 1, 1), -1), date(2024, 12, 1));
        assert_eq!(shift_day(date(2025, 12, 31), 1), date(2026, 1, 1));
    }

    #[test]
    fn zoom_preserves_anchor_and_pan_stays_in_the_day() {
        let mut timeline = Timeline::default();
        assert_eq!(timeline.hour_width(), 160.0);
        let width = 648.0;
        let anchor = 280.0;
        let hour = (timeline.offset + anchor) / timeline.hour_width();
        timeline.zoom_at(125, anchor, width);
        assert!(((timeline.offset + anchor) / timeline.hour_width() - hour).abs() < 0.001);
        timeline.zoom_at(1000, anchor, width);
        assert_eq!(timeline.zoom, 200);
        timeline.zoom_at(0, anchor, width);
        assert_eq!(timeline.zoom, 50);
        timeline.pan(-10000.0, width);
        assert_eq!(timeline.offset, 0.0);
        timeline.pan(10000.0, width);
        assert_eq!(timeline.offset, 24.0 * timeline.hour_width() - width);
        timeline.pan(0.0, 10000.0);
        assert_eq!(timeline.offset, 0.0);
    }
}
