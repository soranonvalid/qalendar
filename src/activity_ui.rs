use crate::{
    activity::{self, Group},
    date_picker::ActivityDatePicker,
    time_range::{TimeRange, animate_picker},
    *,
};
use gpui_kit::{
    base::Switch,
    component::{
        WindowExt as _,
        button::ButtonVariant,
        input::{Input, InputState},
        menu::{ContextMenuExt as _, PopupMenuItem},
        tooltip::Tooltip,
    },
};
use std::time::{Duration, Instant};

const SIDEBAR_BACKGROUND: u32 = 0xfdfdfd;
const ACTIVITY_ACTIONS_CLEARANCE: f32 = 82.0;
const ACTIVITY_ACTION_SPACING: f32 = 56.0;

#[derive(Clone, Copy, PartialEq)]
pub enum ActivityTab {
    Day,
    Manage,
}

pub(crate) struct ActivityMotion {
    target: f32,
    start: Option<(f32, Instant)>,
}

impl ActivityMotion {
    pub(crate) fn new(target: f32) -> Self {
        Self {
            target,
            start: None,
        }
    }

    pub(crate) fn update(
        &mut self,
        target: f32,
        now: Instant,
        duration: Duration,
        reduced: bool,
    ) -> f32 {
        let current = self.start.map_or(self.target, |(from, started)| {
            let distance = self.target - from;
            let step = now.duration_since(started).as_secs_f32() / duration.as_secs_f32();
            if step >= distance.abs() {
                self.target
            } else {
                from + distance.signum() * step
            }
        });
        if reduced || current == target {
            self.start = None;
        } else if self.target != target {
            self.start = Some((current, now));
        }
        self.target = target;
        if reduced { target } else { current }
    }
}

pub(crate) struct DayOdometer {
    target: u32,
    roll: Option<DayRoll>,
}

struct DayRoll {
    days: Vec<u32>,
    started: Instant,
    initial_step: f32,
}

#[derive(Clone, Copy)]
struct DayFrame {
    from: u32,
    to: u32,
    progress: f32,
}

impl DayOdometer {
    const MAX_STEPS: u32 = 6;

    pub(crate) fn new(day: u32) -> Self {
        Self {
            target: day,
            roll: None,
        }
    }

    fn frame(&mut self, now: Instant) -> DayFrame {
        if let Some(roll) = &self.roll {
            let steps = roll.days.len() - 1;
            let duration = Duration::from_millis(150 + 25 * steps as u64);
            let t = now
                .duration_since(roll.started)
                .div_duration_f64(duration)
                .min(1.0);
            let eased = t * t * (3.0 - 2.0 * t);
            let position = roll.initial_step + (steps as f32 - roll.initial_step) * eased as f32;
            let index = position.floor() as usize;
            if index < steps {
                return DayFrame {
                    from: roll.days[index],
                    to: roll.days[index + 1],
                    progress: position.fract(),
                };
            }
            self.roll = None;
        }
        DayFrame {
            from: self.target,
            to: self.target,
            progress: 0.0,
        }
    }

    fn update(&mut self, target: u32, now: Instant, reduced: bool) -> DayFrame {
        if reduced {
            self.target = target;
            self.roll = None;
        } else if target != self.target {
            let frame = self.frame(now);
            let (mut days, initial_step) = if frame.progress == 0.0 {
                (vec![frame.from], 0.0)
            } else {
                let visible =
                    frame.from as f32 + (frame.to as f32 - frame.from as f32) * frame.progress;
                let forward = (target as f32 >= visible) == (frame.to > frame.from);
                if forward {
                    (vec![frame.from, frame.to], frame.progress)
                } else {
                    (vec![frame.to, frame.from], 1.0 - frame.progress)
                }
            };
            let from = *days.last().unwrap();
            let delta = target as i32 - from as i32;
            let steps = delta
                .unsigned_abs()
                .min(Self::MAX_STEPS + 1 - days.len() as u32);
            // Sample long jumps so the wheel does not race through unreadable digits.
            days.extend(
                (1..=steps).map(|step| (from as i32 + delta * step as i32 / steps as i32) as u32),
            );
            self.target = target;
            self.roll = (days.len() > 1).then_some(DayRoll {
                days,
                started: now,
                initial_step,
            });
        }
        self.frame(now)
    }
}

struct DayDigit {
    current: Option<u32>,
    next: Option<u32>,
    progress: f32,
    direction: f32,
}

impl DayDigit {
    fn element(self) -> Div {
        let height = 76.0;
        let edge_opacity = (8.0 * self.progress.min(1.0 - self.progress)).min(1.0);
        let background: Hsla = rgb(SIDEBAR_BACKGROUND).into();
        div()
            .relative()
            .w(px(60.0))
            .h(px(height))
            .flex_shrink_0()
            .overflow_hidden()
            .children(
                [
                    (self.current, -self.progress),
                    (self.next, 1.0 - self.progress),
                ]
                .into_iter()
                .filter(|(_, offset)| offset.abs() < 1.0)
                .filter_map(|(digit, offset)| {
                    digit.map(|digit| {
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(offset * height * self.direction))
                            .h(px(height))
                            .opacity(1.0 - 0.65 * offset.abs())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Text::new_inaccessible(digit.to_string().into()))
                    })
                }),
            )
            .when(edge_opacity > 0.0, |column| {
                column.children([false, true].map(|bottom| {
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .when(bottom, |edge| edge.bottom_0())
                        .when(!bottom, |edge| edge.top_0())
                        .h(px(14.0))
                        .bg(linear_gradient(
                            if bottom { 180.0 } else { 0.0 },
                            linear_color_stop(background.opacity(0.0), 0.0),
                            linear_color_stop(background.opacity(edge_opacity), 1.0),
                        ))
                }))
            })
    }
}

fn day_digits(frame: DayFrame) -> [DayDigit; 2] {
    let DayFrame {
        from: current,
        to: next,
        progress,
    } = frame;
    let direction = if next >= current { 1.0 } else { -1.0 };
    let tens = |day| (day >= 10).then_some(day / 10);
    let carry = if tens(current) == tens(next) {
        0.0
    } else {
        progress + direction * 0.6 * progress * (1.0 - progress)
    };
    [
        DayDigit {
            current: tens(current),
            next: tens(next),
            progress: carry,
            direction,
        },
        DayDigit {
            current: Some(current % 10),
            next: Some(next % 10),
            progress: if current % 10 == next % 10 {
                0.0
            } else {
                progress
            },
            direction,
        },
    ]
}

fn tab_content(progress: f32) -> (ActivityTab, f32, f32) {
    const SWITCH: f32 = 0.45;
    let day = ((progress - SWITCH) / (1.0 - SWITCH)).clamp(0.0, 1.0);
    if progress < SWITCH {
        (ActivityTab::Manage, 1.0 - progress / SWITCH, day)
    } else {
        (ActivityTab::Day, ((progress - SWITCH) / 0.35).min(1.0), day)
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivityMotion, ActivityTab, DayFrame, DayOdometer, day_digits, tab_content};
    use std::time::{Duration, Instant};

    #[test]
    fn activity_motion_reverses_from_the_visible_position() {
        let now = Instant::now();
        let duration = Duration::from_millis(200);
        let mut motion = ActivityMotion::new(0.0);
        assert_eq!(motion.update(1.0, now, duration, false), 0.0);

        let halfway = now + duration / 2;
        assert_eq!(motion.update(0.0, halfway, duration, false), 0.5);
        assert_eq!(
            motion.update(0.0, halfway + duration / 4, duration, false),
            0.25
        );
        assert_eq!(motion.update(0.0, now + duration, duration, false), 0.0);
        assert_eq!(motion.update(1.0, now + duration, duration, true), 1.0);
    }

    #[test]
    fn long_day_rolls_sample_the_route_and_short_rolls_keep_each_day() {
        let start = Instant::now();
        let mut odometer = DayOdometer::new(1);
        odometer.update(31, start, false);
        let days = &odometer.roll.as_ref().unwrap().days;
        assert_eq!(
            (days.first(), days.last(), days.len()),
            (Some(&1), Some(&31), 7)
        );
        assert_eq!(odometer.update(18, start, true).from, 18);
        assert!(odometer.roll.is_none());
        odometer.update(24, start, false);
        assert_eq!(
            odometer.roll.as_ref().unwrap().days,
            [18, 19, 20, 21, 22, 23, 24]
        );
        let end = odometer.frame(start + Duration::from_millis(300));
        assert_eq!((end.from, end.progress), (24, 0.0));
    }

    #[test]
    fn reversing_a_carry_preserves_both_glyph_positions() {
        let start = Instant::now();
        let mut odometer = DayOdometer::new(19);
        odometer.update(20, start, false);
        let halfway = start + Duration::from_millis(85);
        let before = day_digits(odometer.frame(halfway));
        let after = day_digits(odometer.update(19, halfway, false));
        for (before, after) in before.into_iter().zip(after) {
            assert_eq!(before.current, after.next);
            assert_eq!(before.next, after.current);
            assert_eq!(before.direction, -after.direction);
            assert!((before.progress + after.progress - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn day_digits_roll_with_a_tens_lead_at_carry() {
        let digits = |from, to, progress| day_digits(DayFrame { from, to, progress });
        let [tens, ones] = digits(19, 20, 0.5);
        assert_eq!((tens.current, tens.next), (Some(1), Some(2)));
        assert_eq!((ones.current, ones.next), (Some(9), Some(0)));
        assert!(tens.progress > ones.progress);
        assert_eq!(digits(24, 25, 0.5)[0].progress, 0.0);
        assert_eq!(digits(9, 10, 0.0)[0].current, None);
        assert_eq!(digits(9, 10, 0.0)[0].progress, 0.0);
        assert_eq!(digits(10, 10, 0.0)[0].current, Some(1));
        assert_eq!(digits(10, 10, 0.0)[1].current, Some(0));
    }

    #[test]
    fn grouped_list_appears_after_the_day_number_leaves() {
        let (tab, _, day) = tab_content(1.0);
        assert!(tab == ActivityTab::Day);
        assert_eq!(day, 1.0);

        let (_, list, day) = tab_content(0.45);
        assert_eq!((list, day), (0.0, 0.0));

        let (tab, list, day) = tab_content(0.25);
        assert!(tab == ActivityTab::Manage);
        assert!(list > 0.0);
        assert_eq!(day, 0.0);
    }
}

pub struct Draft {
    id: Option<usize>,
    title: Entity<InputState>,
    date: Entity<ActivityDatePicker>,
    pub timed: bool,
    pub time: TimeRange,
    color: Option<u32>,
    error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ActivityDrag {
    id: usize,
    title: String,
    date_label: String,
    color: u32,
}

impl ActivityDrag {
    pub(crate) fn attach<E: StatefulInteractiveElement + Styled>(
        element: E,
        activity: &Activity,
    ) -> E {
        element.cursor_grab().on_drag(
            Self {
                id: activity.id,
                title: activity.title.clone(),
                date_label: activity.date_label(),
                color: activity.color,
            },
            |drag, _, _, cx| cx.new(|_| drag.clone()),
        )
    }
}

impl Render for ActivityDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(200.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(0xffffff))
            .text_color(rgb(INK))
            .font_family(FONT)
            .text_size(px(13.0))
            .flex()
            .gap(px(8.0))
            .child(div().w(px(3.0)).flex_shrink_0().bg(rgb(self.color)))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(div().truncate().child(self.title.clone()))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x707070))
                            .child(self.date_label.clone()),
                    ),
            )
    }
}

impl Qalendar {
    fn edit_activity(&mut self, id: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.is_some() {
            if id.is_some() && self.draft.as_ref().is_some_and(|d| d.id != id) {
                self.draft.as_mut().unwrap().error =
                    Some("Save or cancel this draft before editing another activity.".into());
            }
            self.form_open = true;
            cx.notify();
            return;
        }
        let activity = id
            .and_then(|id| self.activities.iter().find(|a| a.id == id))
            .cloned();
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Activity title")
                .default_value(
                    activity
                        .as_ref()
                        .map(|a| a.title.clone())
                        .unwrap_or_default(),
                )
        });
        let date = activity.as_ref().map_or(self.selected, |a| a.date);
        let date = cx.new(|cx| ActivityDatePicker::new(date, window, cx));
        let time = TimeRange::new(
            activity.as_ref().and_then(|a| a.time),
            activity.as_ref().and_then(|a| a.end_time),
            window,
            cx,
        );
        title.update(cx, |state, cx| state.focus(window, cx));
        self.draft = Some(Draft {
            id,
            title,
            date,
            timed: activity.as_ref().is_some_and(|a| a.time.is_some()),
            time,
            color: activity.as_ref().map(|a| a.color),
            error: None,
        });
        self.preview = None;
        self.form_open = true;
        cx.notify();
    }

    fn close_activity_form(&mut self, discard: bool, cx: &mut Context<Self>) {
        if let Some(draft) = &self.draft {
            draft.date.update(cx, |date, cx| date.close(cx));
        }
        if discard && let Some(draft) = self.draft.take() {
            self.exiting_draft = Some(draft);
        }
        self.form_open = false;
    }

    fn commit_activities(&mut self, activities: Vec<Activity>, cx: &mut Context<Self>) -> bool {
        if self.load_failed {
            return false;
        }
        match activity::save(&self.storage_path, &self.activities, &activities) {
            Ok(()) => {
                self.activities = activities;
                self.storage_error = None;
                cx.notify();
                true
            }
            Err(error) => {
                self.storage_error = Some(error);
                cx.notify();
                false
            }
        }
    }

    pub(crate) fn drop_activity(
        &mut self,
        drag: &ActivityDrag,
        date: NaiveDate,
        time: Option<chrono::NaiveTime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.activities.iter().position(|a| a.id == drag.id) else {
            return;
        };
        let editing = self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id == Some(drag.id));
        if time.is_some() && editing {
            return;
        }
        let activity = &self.activities[index];
        let date_changed = activity.date != date;
        if date_changed || time.is_some_and(|time| activity.time != Some(time)) {
            let mut items = self.activities.clone();
            if let Some(time) = time {
                items[index].schedule_at(date, time);
            } else {
                items[index].reschedule(date);
            }
            if !self.commit_activities(items, cx) {
                return;
            }
            if let Some(draft) = &self.draft
                && editing
            {
                draft
                    .date
                    .update(cx, |picker, cx| picker.set_date(Some(date), window, cx));
            }
        }
        self.selected = date;
        self.preview = None;
        cx.notify();
    }

    fn delete_activity(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let mut items = self.activities.clone();
        items.retain(|a| a.id != id);
        if items.len() == self.activities.len() || !self.commit_activities(items, cx) {
            return;
        }
        if self.draft.as_ref().and_then(|draft| draft.id) == Some(id) {
            self.close_activity_form(true, cx);
        }
        self.preview = None;
        self.focus.focus(window, cx);
    }

    fn confirm_activity_delete(&self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(activity) = self.activities.iter().find(|activity| activity.id == id) else {
            return;
        };
        let title = SharedString::from(activity.title.clone());
        let view = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let activity_title = title.clone();
            alert
                .centered()
                .width(px(320.0))
                .title("Delete this activity?")
                .description(
                    div()
                        .id("delete-activity-title")
                        .role(Role::Label)
                        .aria_value(activity_title.clone())
                        .truncate()
                        .child(activity_title.clone())
                        .tooltip(move |window, cx| {
                            Tooltip::new(activity_title.clone()).build(window, cx)
                        }),
                )
                .confirm()
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    // Restore focus before deletion removes the button or row that opened the dialog.
                    window.close_dialog(cx);
                    let _ = view.update(cx, |this, cx| this.delete_activity(id, window, cx));
                    false
                })
        });
    }

    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.draft else {
            return;
        };
        let date = draft.date.read(cx).date();
        if draft.timed {
            let (start_error, end_error) = draft.time.errors(cx);
            if start_error.is_some() || end_error.is_some() {
                self.draft.as_mut().unwrap().error = None;
                cx.notify();
                return;
            }
        }
        let result = (|| -> Result<Activity, String> {
            let date = date.ok_or("Choose a date.")?;
            let legacy_end = self
                .activities
                .iter()
                .find(|a| draft.id == Some(a.id) && a.date == date)
                .map_or(date, |a| a.legacy_end);
            let (time, end_time) = if draft.timed {
                let (start, end) = draft.time.value(window, cx)?;
                (Some(start), end)
            } else {
                (None, None)
            };
            let mut activity = Activity {
                id: draft.id.unwrap_or(
                    self.activities
                        .iter()
                        .map(|a| a.id)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("Activity ID limit reached.")?,
                ),
                title: draft.title.read(cx).value().trim().to_string(),
                date,
                legacy_end,
                time,
                end_time,
                color: draft.color.unwrap_or_default(),
            };
            activity.validate()?;
            if draft.color.is_none() {
                let grid = month_grid(self.selected);
                activity.color = activity::generate_color(
                    &self.activities,
                    self.last_activity_color,
                    grid[0]..=grid[grid.len() - 1],
                    date,
                    rand::rng(),
                )
                .ok_or("Could not find an unused activity color for these dates.")?;
            }
            Ok(activity)
        })();
        match result {
            Ok(activity) => {
                let generated_color = draft.color.is_none().then_some(activity.color);
                let mut items = self.activities.clone();
                if let Some(old) = items.iter_mut().find(|a| a.id == activity.id) {
                    *old = activity;
                } else {
                    items.push(activity);
                }
                if self.commit_activities(items, cx) {
                    if let Some(color) = generated_color {
                        self.last_activity_color = Some(color);
                    }
                    self.close_activity_form(true, cx);
                    self.focus.focus(window, cx);
                }
            }
            Err(error) => {
                let draft = self.draft.as_mut().unwrap();
                draft.error = Some(error);
            }
        }
        cx.notify();
    }

    fn activity_form(
        &self,
        draft: &Draft,
        visibility: f32,
        editing: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("activity-form")
            .relative()
            .top(px(8.0 * (1.0 - visibility)))
            .opacity(visibility)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .px(px(20.0))
            .pb(px(ACTIVITY_ACTIONS_CLEARANCE + 12.0))
            .child(
                div().flex().flex_col().gap(px(8.0)).child("Title").child(
                    Input::new(&draft.title)
                        .disabled(!editing)
                        .aria_label("Activity title")
                        .h(px(34.0))
                        .rounded(px(12.0))
                        .text_size(px(13.0)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child("Date")
                    .child(if editing {
                        draft.date.clone().into_any_element()
                    } else {
                        draft
                            .date
                            .read(cx)
                            .trigger(cx)
                            .disabled(true)
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child("Time")
                            .child(
                                Switch::new("enable-time")
                                    .accessibility_label("Include time")
                                    .checked(draft.timed)
                                    .disabled(!editing)
                                    .w(px(36.0))
                                    .h(px(20.0))
                                    .p(px(2.0))
                                    .rounded_full()
                                    .cursor_pointer()
                                    .bg(rgb(if draft.timed { BLUE } else { 0xd8d8d8 }))
                                    .focus_visible(|s| s.border_1().border_color(rgb(INK)))
                                    .child(
                                        div()
                                            .size(px(16.0))
                                            .rounded_full()
                                            .bg(rgb(0xffffff))
                                            .when(draft.timed, |d| d.ml(px(16.0))),
                                    )
                                    .on_change({
                                        let view = cx.entity().downgrade();
                                        move |checked, _, _, cx| {
                                            let _ = view.update(cx, |this, cx| {
                                                if let Some(draft) = &mut this.draft {
                                                    draft.timed = checked;
                                                }
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(Self::time_range(draft, editing, cx)),
            )
            .when_some(draft.error.clone(), |form, error| {
                form.child(
                    div()
                        .id("form-error")
                        .role(Role::Alert)
                        .text_size(px(12.0))
                        .text_color(rgb(0xb42318))
                        .child(error),
                )
            })
    }

    fn activity_row(
        &self,
        a: &Activity,
        managed: bool,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = a.id;
        let description = if managed {
            a.date_label()
        } else {
            a.relative_label(Local::now().naive_local())
        };
        let row = div()
            .id(("activity-row", id))
            .relative()
            .flex()
            .items_stretch()
            .gap(px(8.0))
            .w_full()
            .mb(px(6.0))
            .child(div().w(px(3.0)).flex_shrink_0().bg(rgb(a.color)))
            .child(
                button(
                    ("activity", id),
                    format!("Show {} on the calendar", a.title),
                )
                .disabled(!active)
                .flex_1()
                .min_w_0()
                .h_auto()
                .flex_col()
                .items_start()
                .justify_start()
                .gap(px(5.0))
                .py(px(1.0))
                .text_size(px(13.0))
                .line_height(relative(1.12))
                .hover(|s| s.text_color(rgb(0x444444)))
                .when(
                    active
                        && !self.load_failed
                        && (self.view == View::Calendar
                            || self.draft.as_ref().is_none_or(|draft| draft.id != Some(id))),
                    |row| ActivityDrag::attach(row, a),
                )
                .child(
                    div()
                        .w_full()
                        .line_clamp(2)
                        .overflow_hidden()
                        .child(a.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(0x333333))
                        .child(description),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(activity) = this.activities.iter().find(|a| a.id == id) {
                        this.selected = activity.date;
                        this.view = View::Calendar;
                        this.cursor_x = None;
                        this.drag_x = None;
                        this.stop_wheel_scroll();
                        this.preview = None;
                        cx.notify();
                    }
                })),
            );
        if !active {
            return row.into_any_element();
        }

        let view = cx.weak_entity();
        row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered && !cx.has_active_drag() {
                this.preview = Some(id);
            } else if this.preview == Some(id) {
                this.preview = None;
            }
            cx.notify();
        }))
        .when(self.preview == Some(id) && !cx.has_active_drag(), |row| {
            row.child(
                div().absolute().top_0().left_0().child(deferred(
                    anchored()
                        .anchor(Anchor::TopRight)
                        .offset(point(px(-32.0), px(-11.0)))
                        .snap_to_window_with_margin(px(8.0))
                        .child(animate_picker(
                            "activity-preview-enter",
                            Self::preview_card(a),
                        )),
                )),
            )
        })
        .context_menu(move |menu, _, cx| {
            let _ = view.update(cx, |this, cx| {
                this.preview = None;
                cx.notify();
            });
            let edit_view = view.clone();
            let delete_view = view.clone();

            menu.item(PopupMenuItem::new("Edit").icon(IconName::Pencil).on_click(
                move |_, window, cx| {
                    let view = edit_view.clone();
                    window.defer(cx, move |window, cx| {
                        let _ =
                            view.update(cx, |this, cx| this.edit_activity(Some(id), window, cx));
                    });
                },
            ))
            .item(
                PopupMenuItem::element(|_, _| {
                    div()
                        .id("delete-label")
                        .role(Role::Label)
                        .aria_value("Delete")
                        .text_color(rgb(0xb42318))
                        .child("Delete")
                })
                .icon(Icon::new(IconName::Trash).text_color(rgb(0xb42318)))
                .on_click(move |_, window, cx| {
                    let view = delete_view.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = view
                            .update(cx, |this, cx| this.confirm_activity_delete(id, window, cx));
                    });
                }),
            )
        })
        .into_any_element()
    }

    pub fn activity_sidebar(
        &mut self,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let editing = self.form_open;
        let now = Instant::now();
        let reduced = cx.reduce_motion();
        let tab_target = f32::from(self.activity_tab == ActivityTab::Day);
        let form_target = f32::from(editing);
        let tab_progress =
            self.activity_tab_motion
                .update(tab_target, now, Duration::from_millis(240), reduced);
        let form_progress =
            self.activity_form_motion
                .update(form_target, now, Duration::from_millis(180), reduced);
        if editing || form_progress == 0.0 {
            self.exiting_draft = None;
        }
        let displayed_draft = if editing || form_progress > 0.0 {
            self.draft.as_ref().or(self.exiting_draft.as_ref())
        } else {
            None
        };
        let edited_id = displayed_draft.and_then(|draft| draft.id);
        let cancel_slots = if edited_id.is_some() { 2.0 } else { 1.0 };
        if tab_progress != tab_target || form_progress != form_target {
            window.request_animation_frame();
        }
        let (list_tab, list_visibility, day_visibility) = tab_content(tab_progress);
        let roll = ease_in_out(form_progress);
        let day_space = ease_out_quint()(day_visibility) * (1.0 - roll);
        let day_visibility = day_visibility * (1.0 - ease_out_quint()(form_progress));
        let day_frame =
            self.day_odometer
                .update(self.selected.day(), now, reduced || day_space == 0.0);
        if self.day_odometer.roll.is_some() {
            window.request_animation_frame();
        }
        let day_view = list_tab == ActivityTab::Day;
        let list_active = list_tab == self.activity_tab;
        let list_opacity = ease_in_out(list_visibility);
        let list_offset = (1.0 - list_opacity) * if day_view { 14.0 } else { -14.0 };
        let tab_size = 22.0;
        let highlight_target = match self.hovered_activity_tab.unwrap_or(self.activity_tab) {
            ActivityTab::Manage => px(0.0),
            ActivityTab::Day => px(tab_size),
        };
        let today = Local::now().date_naive();
        let shown: Vec<_> = self
            .activities
            .iter()
            .filter(|a| !day_view || a.date == self.selected)
            .collect();
        div()
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(SIDEBAR_BACKGROUND))
            .border_l_1()
            .border_color(rgb(LINE))
            .when(day_space > 0.0, |panel| {
                panel.child(
                    div()
                        .relative()
                        .h(px(124.0 * day_space))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(
                            div()
                                .id("activity-selected-day")
                                .absolute()
                                .top(px(-18.0 * (1.0 - day_visibility)))
                                .left_0()
                                .right_0()
                                .h(px(124.0))
                                .pt(px(8.0))
                                .px(px(20.0))
                                .flex()
                                .items_center()
                                .justify_end()
                                .opacity(day_visibility)
                                .text_size(px(92.0))
                                .line_height(relative(1.0))
                                .font_weight(FontWeight::BLACK)
                                .role(Role::Label)
                                .aria_label(format!("Selected day {}", self.selected.day()))
                                .children(day_digits(day_frame).map(DayDigit::element)),
                        ),
                )
            })
            .child(
                div()
                    .h(px(60.0 - 16.0 * day_space))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(20.0))
                    .child(
                        div()
                            .text_size(px(24.0))
                            .font_weight(FontWeight::BOLD)
                            .child("Activity"),
                    )
                    .child(
                        div()
                            .flex()
                            .p(px(2.0))
                            .border_1()
                            .border_color(rgb(LINE))
                            .rounded_full()
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .child(tab_highlight(
                                        "activity-tab-highlight",
                                        px(tab_size),
                                        px(tab_size),
                                        highlight_target,
                                    ))
                                    .children(
                                        [
                                            (
                                                ActivityTab::Manage,
                                                "clock-mode",
                                                "All activities",
                                                IconName::Clock,
                                            ),
                                            (
                                                ActivityTab::Day,
                                                "activity-mode",
                                                "Selected day activities",
                                                IconName::ListTree,
                                            ),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(tab, id, label, glyph)| {
                                                icon_button(id, label, glyph, tab_size)
                                                    .on_hover(cx.listener(
                                                        move |this, hovered: &bool, _, cx| {
                                                            if *hovered {
                                                                this.hovered_activity_tab =
                                                                    Some(tab);
                                                            } else if this.hovered_activity_tab
                                                                == Some(tab)
                                                            {
                                                                this.hovered_activity_tab = None;
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                                    .aria_toggled((self.activity_tab == tab).into())
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.activity_tab = tab;
                                                        this.close_activity_form(false, cx);
                                                        this.preview = None;
                                                        cx.notify();
                                                    }))
                                            },
                                        ),
                                    ),
                            ),
                    ),
            )
            .when_some(self.storage_error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("storage-error")
                        .px(px(20.0))
                        .pb(px(10.0))
                        .text_size(px(12.0))
                        .text_color(rgb(0xb42318))
                        .role(Role::Alert)
                        .child(error),
                )
            })
            .child(if let Some(draft) = displayed_draft {
                self.activity_form(draft, roll, editing, cx)
                    .into_any_element()
            } else {
                div()
                    .id("activities-scroll")
                    .relative()
                    .left(px(list_offset))
                    .opacity(list_opacity)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .on_scroll(cx.listener(|this, _, _, cx| {
                        if this.preview.take().is_some() {
                            cx.notify();
                        }
                    }))
                    .px(px(20.0))
                    .pb(px(ACTIVITY_ACTIONS_CLEARANCE))
                    .when(shown.is_empty(), |list| {
                        list.child(
                            div()
                                .mt(px(40.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(20.0))
                                .child(icon(IconName::ClockFading, 42.0).text_color(rgb(0x838383)))
                                .child(
                                    div()
                                        .text_center()
                                        .text_size(px(14.0))
                                        .child("Nothing planned yet.")
                                        .child(div().child("Why not make something happen?")),
                                ),
                        )
                    })
                    .when(day_view, |list| {
                        list.children(
                            shown
                                .iter()
                                .map(|a| self.activity_row(a, false, list_active, cx)),
                        )
                    })
                    .when(!day_view, |list| {
                        list.children(Group::ALL.into_iter().filter_map(|group| {
                            let items: Vec<_> =
                                shown.iter().filter(|a| a.group(today) == group).collect();
                            if items.is_empty() {
                                return None;
                            }
                            Some(
                                div()
                                    .mb(px(16.0))
                                    .child(
                                        div()
                                            .mb(px(8.0))
                                            .text_size(px(14.0))
                                            .text_color(rgb(0x707070))
                                            .child(group.label()),
                                    )
                                    .children(
                                        items
                                            .into_iter()
                                            .map(|a| self.activity_row(a, true, list_active, cx)),
                                    ),
                            )
                        }))
                    })
                    .into_any_element()
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(ACTIVITY_ACTIONS_CLEARANCE))
                    .px(px(18.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .when(editing || roll > 0.0, |footer| {
                        footer.child(
                            button("cancel-activity", "Cancel activity changes")
                                .absolute()
                                .top(px(19.0))
                                .right(px(18.0 + ACTIVITY_ACTION_SPACING * cancel_slots * roll))
                                .size(px(44.0))
                                .rounded_full()
                                .block_mouse_except_scroll()
                                .disabled(!editing)
                                .opacity(roll)
                                .bg(rgb(0xdb4b50))
                                .text_color(rgb(0xffffff))
                                .hover(|s| s.bg(rgb(0xbf3339)))
                                .child(
                                    icon(IconName::X, 24.2)
                                        .rotate(radians(-2.55 * cancel_slots * (1.0 - roll))),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_activity_form(true, cx);
                                    this.focus.focus(window, cx);
                                    cx.notify();
                                })),
                        )
                    })
                    .when_some(edited_id, |footer, id| {
                        footer.child(
                            button("delete-activity", "Delete activity")
                                .absolute()
                                .top(px(19.0))
                                .right(px(18.0 + ACTIVITY_ACTION_SPACING * roll))
                                .size(px(44.0))
                                .rounded_full()
                                .block_mouse_except_scroll()
                                .disabled(!editing || self.load_failed)
                                .opacity(roll)
                                .bg(rgb(0xf47a2e))
                                .text_color(rgb(0xffffff))
                                .hover(|s| s.bg(rgb(0xb84b10)))
                                .child(
                                    icon(IconName::Trash, 24.2)
                                        .rotate(radians(-2.55 * (1.0 - roll))),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_activity_delete(id, window, cx);
                                })),
                        )
                    })
                    .child(
                        button(
                            "create-activity",
                            if editing {
                                "Save activity"
                            } else if self.draft.is_some() {
                                "Resume activity draft"
                            } else {
                                "Create activity"
                            },
                        )
                        .relative()
                        .size(px(44.0))
                        .rounded_full()
                        .block_mouse_except_scroll()
                        .disabled(self.load_failed)
                        .bg(rgb(BLUE))
                        .text_color(rgb(0xffffff))
                        .hover(|s| s.bg(rgb(0x1d5bd0)))
                        .children(
                            [(IconName::Plus, 1.0 - roll), (IconName::Check, roll)]
                                .into_iter()
                                .filter(|(_, opacity)| *opacity > 0.0)
                                .map(|(glyph, opacity)| {
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .opacity(opacity)
                                        .child(icon(glyph, 24.2))
                                }),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if editing {
                                    this.save_draft(window, cx);
                                } else {
                                    this.edit_activity(None, window, cx);
                                }
                            },
                        )),
                    ),
            )
    }

    fn preview_card(a: &Activity) -> Div {
        div()
            .w(px(if a.time.is_some() { 284.0 } else { 250.0 }))
            .p(px(12.0))
            .rounded(px(20.0))
            .border_1()
            .border_color(rgb(0xe6e6e6))
            .bg(rgb(0xfafafa))
            .flex()
            .items_stretch()
            .gap(px(12.0))
            .child(div().w(px(6.0)).flex_shrink_0().bg(rgb(a.color)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .line_height(relative(1.15))
                            .line_clamp(6)
                            .overflow_hidden()
                            .child(a.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .whitespace_nowrap()
                            .gap(px(10.0))
                            .text_size(px(11.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(icon(IconName::Calendar, 13.0))
                                    .child(a.date_label()),
                            )
                            .when_some(a.time_label(), |d, time| {
                                d.child(
                                    div()
                                        .flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(icon(IconName::Clock, 13.0))
                                        .child(time),
                                )
                            }),
                    ),
            )
    }
}
