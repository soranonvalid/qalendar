#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod activity;
mod activity_ui;
mod calendar;
mod date_picker;
mod time_range;

use activity::Activity;
use activity_ui::{ActivityDrag, ActivityMotion, ActivityTab, DayOdometer, Draft};
use calendar::{TimeBlock, Timeline, month_grid, shift_day, shift_month, time_blocks};
use chrono::{Datelike, Local, NaiveDate, NaiveTime};
use gpui_kit::{
    assets::IconName,
    base::Button,
    component::{Icon, Sizable, Theme, ThemeMode},
    prelude::FluentBuilder,
    *,
};
use std::time::{Duration, Instant};

const LINE: u32 = 0xeeeeee;
const INK: u32 = 0x090909;
const BLUE: u32 = 0x3072f6;
const FONT: &str = "Archivo";
const WINDOW_WIDTH: f32 = 864.0;
const WINDOW_HEIGHT: f32 = 614.0;
const MIN_SIDEBAR_WIDTH: f32 = 217.0;
const TOP_BAR_HEIGHT: f32 = 37.0;
const CALENDAR_HEADER_HEIGHT: f32 = 65.0;
const WEEKDAY_HEADER_HEIGHT: f32 = 34.0;
const VISIBLE_ACTIVITY_TITLES: usize = 3;
const TIME_BLOCK_TOP: f32 = 66.0;
const TIME_BLOCK_BOTTOM: f32 = 60.0;
const TIME_BLOCK_HEIGHT: f32 = 72.0;
const TIME_BLOCK_LANE_HEIGHT: f32 = TIME_BLOCK_HEIGHT + 8.0;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Calendar,
    Time,
}

struct Qalendar {
    selected: NaiveDate,
    view: View,
    hovered_view: Option<View>,
    month_transition: MonthTransition,
    view_motion: ActivityMotion,
    timeline: Timeline,
    wheel_scroll: Option<(f32, std::time::Instant)>,
    lane_scroll: f32,
    cursor_x: Option<f32>,
    drag_x: Option<f32>,
    focus: FocusHandle,
    date_focus: FocusHandle,
    timeline_focus: FocusHandle,
    activities: Vec<Activity>,
    last_activity_color: Option<u32>,
    activity_tab: ActivityTab,
    hovered_activity_tab: Option<ActivityTab>,
    activity_tab_motion: ActivityMotion,
    activity_form_motion: ActivityMotion,
    day_odometer: DayOdometer,
    draft: Option<Draft>,
    exiting_draft: Option<Draft>,
    form_open: bool,
    preview: Option<usize>,
    storage_path: std::path::PathBuf,
    storage_error: Option<String>,
    load_failed: bool,
}

const MONTH_SLIDE_DURATION: Duration = Duration::from_millis(270);

fn month_key(date: NaiveDate) -> (i32, u32) {
    (date.year(), date.month())
}

struct MonthTransition {
    date: NaiveDate,
    outgoing: Option<(NaiveDate, Instant)>,
}

impl MonthTransition {
    fn update(&mut self, date: NaiveDate, now: Instant, reduced: bool) -> Option<MonthSlide> {
        self.outgoing = if reduced {
            None
        } else if month_key(self.date) == month_key(date) {
            self.outgoing
        } else {
            let start = self
                .outgoing
                .filter(|(previous, _)| month_key(*previous) == month_key(date))
                .map(|(_, start)| {
                    let elapsed = (now - start).div_duration_f64(MONTH_SLIDE_DURATION);
                    let progress = ease_out_quint()(elapsed.min(1.0) as f32);
                    // Invert the easing so reversing starts at the visible position.
                    let reverse_elapsed = 1.0 - f64::from(progress).powf(0.2);
                    now - MONTH_SLIDE_DURATION.mul_f64(reverse_elapsed)
                })
                .unwrap_or(now);
            Some((self.date, start))
        };
        self.date = date;
        let (previous, start) = self.outgoing?;
        let elapsed = now - start;
        if elapsed >= MONTH_SLIDE_DURATION {
            self.outgoing = None;
            return None;
        }
        let progress = ease_out_quint()(elapsed.div_duration_f64(MONTH_SLIDE_DURATION) as f32);
        Some(MonthSlide {
            previous,
            progress,
            direction: if date > previous { 1.0 } else { -1.0 },
        })
    }
}

#[derive(Clone, Copy)]
struct MonthSlide {
    previous: NaiveDate,
    progress: f32,
    direction: f32,
}

#[cfg(test)]
mod month_motion_tests {
    use super::{MONTH_SLIDE_DURATION, MonthTransition};
    use chrono::NaiveDate;
    use std::time::Instant;

    #[test]
    fn month_slide_reverses_through_the_same_position() {
        let november = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let december = NaiveDate::from_ymd_opt(2026, 12, 16).unwrap();
        let mut transition = MonthTransition {
            date: november,
            outgoing: None,
        };
        let start = Instant::now();
        assert!(transition.update(november, start, false).is_none());
        transition.update(december, start, false).unwrap();

        let now = start + MONTH_SLIDE_DURATION / 4;
        let forward = transition.update(december, now, false).unwrap();
        assert!((forward.progress - (1.0 - 0.75_f32.powi(5))).abs() < 1e-6);
        let backward = transition.update(november, now, false).unwrap();
        assert_eq!(forward.previous, november);
        assert_eq!(backward.previous, december);
        assert_eq!(forward.direction, -backward.direction);
        assert!((forward.progress + backward.progress - 1.0).abs() < 1e-6);
        assert!(transition.update(december, now, true).is_none());
    }
}

fn title_slide(outgoing: String, incoming: String, slide: Option<MonthSlide>) -> impl IntoElement {
    let (label, offset, opacity) = match slide.filter(|_| outgoing != incoming) {
        Some(slide) if slide.progress < 0.5 => {
            let progress = 2.0 * slide.progress;
            (outgoing, -12.0 * slide.direction * progress, 1.0 - progress)
        }
        Some(slide) => {
            let progress = 2.0 * slide.progress - 1.0;
            (
                incoming,
                12.0 * slide.direction * (1.0 - progress),
                progress,
            )
        }
        None => (incoming, 0.0, 1.0),
    };
    div().overflow_hidden().child(
        div()
            .relative()
            .top(px(offset))
            .opacity(opacity)
            .child(label),
    )
}

fn icon(name: IconName, size: f32) -> Icon {
    Icon::new(name).with_size(px(size))
}

fn activity_badge_scale(calendar_width: f32, window_height: f32) -> f32 {
    (calendar_width / (WINDOW_WIDTH - MIN_SIDEBAR_WIDTH))
        .min(window_height / WINDOW_HEIGHT)
        .clamp(0.75, 2.0)
}

fn show_activity_titles(calendar_width: f32, window_height: f32, weeks: usize) -> bool {
    let grid_height =
        window_height - TOP_BAR_HEIGHT - CALENDAR_HEADER_HEIGHT - WEEKDAY_HEADER_HEIGHT;
    calendar_width / 7.0 >= 140.0 && grid_height / weeks as f32 >= 146.0
}

fn activity_pill(scale: f32) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(20.0 * scale))
        .pl(px(5.0 * scale))
        .pr(px(8.0 * scale))
        .rounded_full()
        .border_1()
        .border_color(rgb(0xe4e4e4))
        .bg(rgb(0xffffff))
        .text_color(rgb(INK))
}

fn activity_dot(color: u32, scale: f32) -> Div {
    div()
        .flex_shrink_0()
        .size(px(14.0 * scale))
        .rounded_full()
        .bg(rgb(color))
}

fn activity_count_pill<'a>(
    count: usize,
    activities: impl Iterator<Item = &'a Activity>,
    scale: f32,
) -> Div {
    activity_pill(scale)
        .children(activities.enumerate().map(|(index, activity)| {
            activity_dot(activity.color, scale).when(index > 0, |dot| dot.ml(px(-5.0 * scale)))
        }))
        .child(
            div()
                .ml(px(5.0 * scale))
                .text_size(px(10.0 * scale))
                .child(count.to_string()),
        )
}

fn activity_title_pills<'a>(
    count: usize,
    activities: impl Iterator<Item = &'a Activity> + Clone,
    can_drag: bool,
) -> Div {
    let hidden = count.saturating_sub(VISIBLE_ACTIVITY_TITLES);
    div()
        .absolute()
        .bottom(px(14.0))
        .left(px(10.0))
        .right(px(10.0))
        .max_w(px(168.0))
        .flex()
        .flex_col()
        .items_start()
        .gap(px(5.0))
        .when(hidden > 0, |stack| {
            stack.child(
                activity_count_pill(
                    hidden,
                    activities.clone().skip(VISIBLE_ACTIVITY_TITLES).take(2),
                    1.0,
                )
                .border_color(rgb(0xdcdcdc)),
            )
        })
        .children(activities.take(VISIBLE_ACTIVITY_TITLES).map(|activity| {
            activity_pill(1.0)
                .id(("calendar-activity", activity.id))
                .max_w(relative(1.0))
                .gap(px(5.0))
                .border_color(rgb(0xdcdcdc))
                .text_size(px(14.0))
                .font_weight(FontWeight::NORMAL)
                .child(activity_dot(activity.color, 1.0))
                .child(div().min_w_0().truncate().child(activity.title.clone()))
                .when(can_drag, |pill| ActivityDrag::attach(pill, activity))
        }))
}

fn timeline_block(block: TimeBlock<'_>, offset: f32, width: f32) -> Option<impl IntoElement> {
    let left = (block.left - offset).max(0.0);
    let right = (block.right - offset).min(width);
    if right <= left {
        return None;
    }
    let activity = block.activity;
    let start = activity.time?;
    let point = activity.end_time.is_none();
    let time = activity
        .time_label()
        .expect("timeline blocks have a start time");
    let label = format!("{}, {time}", activity.title);
    let block_width = right - left;
    let visible_end = activity.end_time.filter(|&end| {
        let min_width = if end <= start { 160.0 } else { 110.0 };
        block_width >= min_width
    });
    Some(
        div()
            .id(("time-block", activity.id))
            .absolute()
            .left(px(left))
            .top(px(block.lane as f32 * TIME_BLOCK_LANE_HEIGHT))
            .w(px(block_width))
            .h(px(if point { 16.0 } else { TIME_BLOCK_HEIGHT }))
            .rounded(px(7.0))
            .bg(if point {
                rgb(activity.color)
            } else {
                rgba((activity.color << 8) | 0x26)
            })
            .when(!point, |card| {
                card.border_l_1().border_color(rgb(activity.color))
            })
            .overflow_hidden()
            .role(Role::Label)
            .aria_label(label)
            .when(!point && block_width >= 42.0, |card| {
                card.flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.0))
                    .px(px(10.0))
                    .text_size(px(17.0))
                    .line_height(relative(1.0))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(activity.title.clone()),
                    )
                    .when_some(visible_end, |card, end| {
                        card.child(
                            div()
                                .flex()
                                .justify_between()
                                .gap(px(8.0))
                                .whitespace_nowrap()
                                .text_size(px(14.0))
                                .child(start.format("%H:%M").to_string())
                                .child(activity::end_time_label(start, end)),
                        )
                    })
            }),
    )
}

#[cfg(test)]
mod activity_badge_tests {
    use super::{activity_badge_scale, show_activity_titles};

    #[test]
    fn activity_titles_require_room_in_each_cell() {
        assert!(!show_activity_titles(647.0, 614.0, 5));
        assert!(!show_activity_titles(979.0, 1080.0, 5));
        assert!(show_activity_titles(980.0, 866.0, 5));
        assert!(!show_activity_titles(980.0, 866.0, 6));
        assert!(show_activity_titles(980.0, 1012.0, 6));
    }

    #[test]
    fn scales_with_the_available_width_and_height() {
        assert_eq!(activity_badge_scale(647.0, 614.0), 1.0);
        assert!((activity_badge_scale(1560.0, 1080.0) - 1.76).abs() < 0.01);
        assert_eq!(activity_badge_scale(1560.0, 614.0), 1.0);
        assert_eq!(activity_badge_scale(2500.0, 1600.0), 2.0);
    }
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .accessibility_label(label)
        .cursor_pointer()
        .border_1()
        .border_color(transparent_black())
        .focus_visible(|s| s.border_color(rgb(BLUE)))
}

fn icon_button(id: &'static str, label: &'static str, name: IconName, size: f32) -> Button {
    button(id, label)
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .child(icon(name, size * 0.55))
}

fn tab_highlight(
    id: &'static str,
    width: Pixels,
    height: Pixels,
    target: Pixels,
) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .w(width)
        .h(height)
        .rounded_full()
        .bg(rgb(0xf0f0f0))
        .with_spring(
            id,
            SpringAnimation::new(SpringConfig::new(2500.0, 100.0, 1.0)).to(target),
            |pill, offset| pill.left(offset),
        )
}

fn toggle_maximize(window: &Window) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{SW_MAXIMIZE, SW_RESTORE, ShowWindowAsync},
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            // GPUI's Windows zoom only maximizes; explicitly restore for both click and keyboard activation.
            let command = if window.is_maximized() {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            };
            // SAFETY: this handle belongs to the live GPUI window on the UI thread.
            if let Err(error) =
                unsafe { ShowWindowAsync(HWND(handle.hwnd.get() as *mut _), command) }.ok()
            {
                eprintln!("Unable to change window size: {error}");
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    window.zoom_window();
}

impl Qalendar {
    fn navigate(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.selected = match self.view {
            View::Calendar => shift_month(self.selected, delta),
            View::Time => shift_day(self.selected, delta),
        };
        cx.notify();
    }

    fn reset(&mut self, window: &Window, cx: &mut Context<Self>) {
        let now = Local::now().naive_local();
        self.selected = now.date();
        self.timeline = Timeline::default();
        let width = Self::main_width(window);
        if let Some(x) = self.timeline.current_time_x(self.selected, now) {
            self.timeline.offset = x - width / 2.0;
            self.timeline.pan(0.0, width);
        }
        self.wheel_scroll = None;
        cx.notify();
    }

    fn main_width(window: &Window) -> f32 {
        let width = f32::from(window.viewport_size().width);
        width - (width * 0.25).clamp(MIN_SIDEBAR_WIDTH, 360.0)
    }

    fn zoom(&mut self, value: i32, window: &Window, cx: &mut Context<Self>) {
        self.stop_wheel_scroll();
        let width = Self::main_width(window);
        self.timeline.zoom_at(value, width / 2.0, width);
        cx.notify();
    }

    fn stop_wheel_scroll(&mut self) {
        if let Some((offset, _)) = self.wheel_scroll.take() {
            self.timeline.offset = offset;
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Form controls own their keys, including letters and arrow navigation.
        if self.form_open
            || !(self.focus.is_focused(window)
                || self.date_focus.is_focused(window)
                || self.timeline_focus.is_focused(window))
        {
            return;
        }
        let key = event.keystroke.key.as_str();
        match key {
            "pageup" => self.navigate(-1, cx),
            "pagedown" => self.navigate(1, cx),
            "home" => self.reset(window, cx),
            "c" => {
                self.view = View::Calendar;
                self.cursor_x = None;
                cx.notify();
            }
            "t" => {
                self.view = View::Time;
                cx.notify();
            }
            "+" | "=" if self.view == View::Time => self.zoom(self.timeline.zoom + 25, window, cx),
            "-" if self.view == View::Time => self.zoom(self.timeline.zoom - 25, window, cx),
            "0" if self.view == View::Time => self.zoom(100, window, cx),
            "left" | "right" | "up" | "down" => {
                if self.view == View::Calendar {
                    let delta = match key {
                        "left" => -1,
                        "right" => 1,
                        "up" => -7,
                        _ => 7,
                    };
                    self.selected = shift_day(self.selected, delta);
                    self.date_focus.focus(window, cx);
                } else {
                    self.stop_wheel_scroll();
                    let delta = if key == "left" || key == "up" {
                        -80.0
                    } else {
                        80.0
                    };
                    self.timeline.pan(delta, Self::main_width(window));
                }
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn top_bar(&self) -> impl IntoElement {
        let control = 24.0;
        div()
            .h(px(TOP_BAR_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(12.0))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(
                icon_button(
                    "toggle-sidebar",
                    "Toggle sidebar",
                    IconName::PanelRightClose,
                    30.0,
                )
                .disabled(true)
                .cursor_default(),
            )
            .child(
                div()
                    .id("window-drag")
                    .flex_1()
                    .h_full()
                    .when(cfg!(target_os = "windows"), |area| {
                        area.window_control_area(WindowControlArea::Drag)
                    })
                    .when(!cfg!(target_os = "windows"), |area| {
                        area.on_mouse_down(MouseButton::Left, |event, window, _| {
                            if event.click_count == 2 {
                                toggle_maximize(window);
                            } else {
                                window.start_window_move();
                            }
                        })
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.0))
                    .child(
                        icon_button("minimize", "Minimize window", IconName::Minimize2, control)
                            .hover(|s| s.bg(rgb(0xf0f0f0)))
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                    .child(
                        icon_button(
                            "maximize",
                            "Maximize or restore window",
                            IconName::Maximize,
                            control,
                        )
                        .hover(|s| s.bg(rgb(0xf0f0f0)))
                        .on_click(|_, window, _| toggle_maximize(window)),
                    )
                    .child(
                        icon_button("close", "Close window", IconName::X, control)
                            .hover(|s| s.bg(rgb(0xffe7e7)).text_color(rgb(0xbe2020)))
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }

    fn header(&self, slide: Option<MonthSlide>, cx: &mut Context<Self>) -> impl IntoElement {
        let control_size = 28.0;
        let tab_width = px(100.0);
        let tab_height = px(28.0);
        let previous = slide.map_or(self.selected, |slide| slide.previous);
        let highlight_target = match self.hovered_view.unwrap_or(self.view) {
            View::Calendar => px(0.0),
            View::Time => tab_width,
        };
        div()
            .h(px(CALENDAR_HEADER_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(12.0))
            .gap(px(12.0))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .text_size(px(36.0))
                    .line_height(relative(1.0))
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex()
                            .font_weight(FontWeight::BOLD)
                            .child(title_slide(
                                previous.format("%B").to_string(),
                                self.selected.format("%B").to_string(),
                                slide,
                            )),
                    )
                    .child(",")
                    .child(div().ml(px(9.0)).child(title_slide(
                        previous.year().to_string(),
                        self.selected.year().to_string(),
                        slide,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .p(px(2.0))
                            .border_1()
                            .border_color(rgb(LINE))
                            .rounded_full()
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .child(tab_highlight(
                                        "view-highlight",
                                        tab_width,
                                        tab_height,
                                        highlight_target,
                                    ))
                                    .children(
                                        [
                                            (View::Calendar, "Calendar", IconName::Calendar),
                                            (View::Time, "Time", IconName::CalendarDays),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(view, label, glyph)| {
                                                button(label, label)
                                                    .on_hover(cx.listener(
                                                        move |this, hovered: &bool, _, cx| {
                                                            if *hovered {
                                                                this.hovered_view = Some(view);
                                                            } else if this.hovered_view
                                                                == Some(view)
                                                            {
                                                                this.hovered_view = None;
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                                    .w(tab_width)
                                                    .h(tab_height)
                                                    .gap(px(5.0))
                                                    .rounded_full()
                                                    .text_size(px(12.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(if self.view == view {
                                                        INK
                                                    } else {
                                                        0x666666
                                                    }))
                                                    .aria_toggled((self.view == view).into())
                                                    .child(icon(glyph, 15.0))
                                                    .child(label)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.view = view;
                                                        this.cursor_x = None;
                                                        this.drag_x = None;
                                                        cx.notify();
                                                    }))
                                            },
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                icon_button(
                                    "previous",
                                    "Previous (Page Up)",
                                    IconName::ArrowDown,
                                    control_size,
                                )
                                .bg(rgb(0x000000))
                                .text_color(rgb(0xffffff))
                                .hover(|s| s.bg(rgb(0x303030)))
                                .on_click(cx.listener(|this, _, _, cx| this.navigate(-1, cx))),
                            )
                            .child(
                                icon_button(
                                    "reset",
                                    "Return to today (Home)",
                                    IconName::RotateCcwClock,
                                    26.0,
                                )
                                .hover(|s| s.bg(rgb(0xf0f0f0)))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.reset(window, cx)),
                                ),
                            )
                            .child(
                                icon_button(
                                    "next",
                                    "Next (Page Down)",
                                    IconName::ArrowUp,
                                    control_size,
                                )
                                .bg(rgb(0x000000))
                                .text_color(rgb(0xffffff))
                                .hover(|s| s.bg(rgb(0x303030)))
                                .on_click(cx.listener(|this, _, _, cx| this.navigate(1, cx))),
                            ),
                    ),
            )
    }

    fn calendar(
        &self,
        width: f32,
        window_height: f32,
        slide: Option<MonthSlide>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(px(WEEKDAY_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .children(
                        ["Su", "Mo", "Tu", "We", "Th", "Fr", "St"]
                            .into_iter()
                            .enumerate()
                            .map(|(column, day)| {
                                div()
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::BOLD)
                                    .border_b_1()
                                    .when(column < 6, |cell| cell.border_r_1())
                                    .border_color(rgb(LINE))
                                    .child(day)
                            }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when_some(slide, |grid, slide| {
                        grid.child(
                            div()
                                .id("outgoing-month")
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w_full()
                                .flex()
                                .flex_col()
                                .left(px(-slide.direction * slide.progress * width))
                                .child(self.calendar_days(
                                    slide.previous,
                                    width,
                                    window_height,
                                    false,
                                    cx,
                                )),
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .flex()
                            .flex_col()
                            .left(px(slide.map_or(0.0, |slide| {
                                slide.direction * (1.0 - slide.progress) * width
                            })))
                            .child(self.calendar_days(
                                self.selected,
                                width,
                                window_height,
                                true,
                                cx,
                            )),
                    ),
            )
    }

    fn calendar_days(
        &self,
        selection: NaiveDate,
        width: f32,
        window_height: f32,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dates = month_grid(selection);
        let badge_scale = activity_badge_scale(width, window_height);
        let show_titles = show_activity_titles(width, window_height, dates.len() / 7);
        let can_reschedule = active && !self.load_failed;
        let today = Local::now().date_naive();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .children(dates.chunks(7).map(|week| {
                div()
                    .flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .children(week.iter().enumerate().map(|(column, &date)| {
                        let selected = date == selection;
                        let activities = self.activities.iter().filter(|a| a.date == date);
                        let count = activities.clone().count();
                        let date_label = date.format("%A, %B %-d, %Y");
                        let mut label = match count {
                            0 => date_label.to_string(),
                            1 => format!("{date_label}, 1 activity"),
                            _ => format!("{date_label}, {count} activities"),
                        };
                        if show_titles {
                            for activity in activities.clone().take(VISIBLE_ACTIVITY_TITLES) {
                                label.push_str(", ");
                                label.push_str(&activity.title);
                            }
                        }
                        Button::new(("day", date.num_days_from_ce() as usize))
                            .accessibility_label(label)
                            .disabled(!active)
                            .when(active, |b| {
                                b.cursor_pointer().hover(|s| s.bg(rgb(0xf0f0f0)))
                            })
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .relative()
                            .items_start()
                            .justify_start()
                            .p(px(6.0))
                            .border_0()
                            .when(column < 6, |cell| cell.border_r_1())
                            .border_b_1()
                            .border_color(rgb(if show_titles { 0xe8e8e8 } else { LINE }))
                            .bg(rgb(if selected { 0xf7f7f7 } else { 0xffffff }))
                            .text_color(rgb(if date == today {
                                0xff404d
                            } else if date.month() == selection.month() {
                                0x414141
                            } else {
                                0xa4a4a4
                            }))
                            .text_size(px(28.0))
                            .when(show_titles, |cell| {
                                cell.px(px(10.0)).py(px(8.0)).text_size(px(26.0))
                            })
                            .font_weight(FontWeight::BOLD)
                            .tab_stop(active && selected)
                            .aria_selected(active && selected)
                            .when(active && selected, |b| b.track_focus(&self.date_focus))
                            .focus_visible(|s| s.bg(rgb(0xeaf1ff)).border_color(rgb(BLUE)))
                            .child(date.day().to_string())
                            .when(count > 0, |cell| {
                                if show_titles {
                                    return cell.child(activity_title_pills(
                                        count,
                                        activities,
                                        can_reschedule,
                                    ));
                                }
                                cell.child(
                                    activity_count_pill(count, activities.take(3), badge_scale)
                                        .absolute()
                                        .bottom(px(6.0 * badge_scale))
                                        .left(px(8.0 * badge_scale)),
                                )
                            })
                            .when(can_reschedule, |cell| {
                                cell.drag_over::<ActivityDrag>(|style, _, _, _| {
                                    style.bg(rgb(0xeaf1ff))
                                })
                                .on_drop(cx.listener(
                                    move |this, drag: &ActivityDrag, window, cx| {
                                        this.drop_activity(drag, date, None, window, cx);
                                    },
                                ))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = date;
                                if !this.form_open {
                                    this.activity_tab = ActivityTab::Day;
                                }
                                cx.notify();
                            }))
                    }))
            }))
    }

    fn time_view(
        &mut self,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.timeline.pan(0.0, width);
        let hour_width = self.timeline.hour_width();
        let mut offset = self.timeline.offset;
        if let Some((previous, frame)) = self.wheel_scroll.take() {
            let now = std::time::Instant::now();
            let remaining = (previous - offset) * (-32.0 * (now - frame).as_secs_f32()).exp();
            if remaining.abs() > 0.5 && !cx.reduce_motion() {
                offset = (offset + remaining).clamp(0.0, (24.0 * hour_width - width).max(0.0));
                self.wheel_scroll = Some((offset, now));
                window.request_animation_frame();
            }
        }
        let zoom = self.timeline.zoom;
        let label_scale = zoom.min(100) as f32 / 100.0;
        let label_width = 116.0 * label_scale;
        let marker_x = self
            .timeline
            .current_time_x(self.selected, Local::now().naive_local())
            .map(|x| x - offset)
            .filter(|x| (0.0..width).contains(x));
        let cursor_x = self.cursor_x.filter(|x| (0.0..width).contains(x));
        let blocks = time_blocks(&self.activities, self.selected, hour_width);
        let lanes = blocks.iter().map(|block| block.lane + 1).max().unwrap_or(0);
        let lanes_height = lanes as f32 * TIME_BLOCK_LANE_HEIGHT;
        let lane_top = TOP_BAR_HEIGHT + CALENDAR_HEADER_HEIGHT + TIME_BLOCK_TOP;
        let lane_bottom = f32::from(window.viewport_size().height) - TIME_BLOCK_BOTTOM;
        let max_lane_scroll = (lanes_height - (lane_bottom - lane_top).max(0.0)).max(0.0);
        self.lane_scroll = self.lane_scroll.min(max_lane_scroll);
        let lane_scroll = self.lane_scroll;
        div().id("timeline").relative().flex_1().min_h_0().w_full().overflow_hidden()
            .track_focus(&self.timeline_focus).tab_index(0).tab_stop(true)
            .role(Role::Group).aria_label("Time ruler")
            .aria_description("Drag or scroll to pan. Control plus scroll, or plus and minus keys, to zoom. Zero resets zoom. Page Up and Page Down scroll stacked activities.")
            .cursor_grab().focus_visible(|s| s.bg(rgb(0xfcfdff)))
            .when(!self.load_failed, |ruler| {
                ruler.drag_over::<ActivityDrag>(|style, _, _, _| style.bg(rgb(0xf4f8ff)))
                    .on_drop(cx.listener(move |this, drag: &ActivityDrag, window, cx| {
                        let x = offset + f32::from(window.mouse_position().x);
                        if (0.0..24.0 * hour_width).contains(&x) {
                            if let Some(time) = NaiveTime::from_num_seconds_from_midnight_opt(
                                Timeline::minute_at(x, hour_width) * 60, 0,
                            ) {
                                this.drop_activity(drag, this.selected, Some(time), window, cx);
                            }
                        }
                    }))
            })
            .when(max_lane_scroll > 0.0, |ruler| ruler.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let page = (lane_bottom - lane_top).max(0.0);
                let distance = match event.keystroke.key.as_str() {
                    "pageup" => -page,
                    "pagedown" => page,
                    _ => return,
                };
                this.lane_scroll = (this.lane_scroll + distance).clamp(0.0, max_lane_scroll);
                cx.stop_propagation();
                cx.notify();
            })))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.timeline_focus.focus(window, cx);
                this.stop_wheel_scroll();
                this.drag_x = Some(f32::from(e.position.x));
            }))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                let x = f32::from(e.position.x);
                if this.cursor_x.replace(x) != Some(x) {
                    cx.notify();
                }
                if e.dragging() {
                    if let Some(previous) = this.drag_x.replace(x) {
                        this.timeline.pan(previous - x, width);
                    }
                } else { this.drag_x = None; }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.drag_x = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.drag_x = None))
            .on_scroll(cx.listener(move |this, e: &ScrollEvent, _, cx| {
                let delta = e.delta.pixel_delta(px(32.0));
                if e.modifiers.control {
                    this.stop_wheel_scroll();
                    let direction = if delta.y > px(0.0) { 25 } else if delta.y < px(0.0) { -25 } else { 0 };
                    this.timeline.zoom_at(this.timeline.zoom + direction, f32::from(e.position.x).clamp(0.0, width), width);
                } else if max_lane_scroll > 0.0
                    && (lane_top..lane_bottom).contains(&f32::from(e.position.y))
                    && delta.y.abs() > delta.x.abs()
                {
                    this.lane_scroll =
                        (this.lane_scroll - f32::from(delta.y)).clamp(0.0, max_lane_scroll);
                } else {
                    let distance = if delta.x != px(0.0) { delta.x } else { delta.y };
                    if matches!(e.delta, ScrollDelta::Lines(_)) && !cx.reduce_motion() {
                        this.wheel_scroll.get_or_insert((this.timeline.offset, std::time::Instant::now()));
                    } else {
                        this.stop_wheel_scroll();
                    }
                    this.timeline.pan(-f32::from(distance), width);
                }
                cx.stop_propagation(); cx.notify();
            }))
            .children((0..=96).filter_map(move |quarter| {
                let x = quarter as f32 * hour_width / 4.0 - offset;
                if x < 0.0 || x >= width { return None; }
                Some(div().absolute().left(px(x)).top_0().bottom_0().w(px(4.0)).border_l_1()
                    .border_color(rgb(if quarter % 4 == 0 { 0xe0e0e0 } else { 0xebebeb }))
                    .when(quarter % 4 != 0, |line| line.border_dashed()))
            }))
            .children((0..24).filter_map(move |hour| {
                let left = Timeline::label_left(hour, hour_width, label_width, offset);
                let opacity = Timeline::label_opacity(left, label_width, width);
                if opacity == 0.0 { return None; }
                Some(div().absolute().left(px(left)).top(px(9.0)).w(px(label_width))
                    .opacity(opacity)
                    .text_center().text_size(px(42.0 * label_scale)).line_height(relative(1.0))
                    .font_weight(FontWeight::BOLD).child(format!("{hour:02}:00")))
            }))
            .child(div().absolute().left_0().right_0().top(px(TIME_BLOCK_TOP))
                .bottom(px(TIME_BLOCK_BOTTOM)).overflow_hidden()
                .child(div().relative().top(px(-lane_scroll)).w_full().h(px(lanes_height))
                    .children(blocks.into_iter().filter_map(|block| timeline_block(block, offset, width)))))
            .when_some(cursor_x, |d, x| {
                d.child(div().absolute().inset_0().with_spring(
                    "cursor-guide",
                    SpringAnimation::new(SpringConfig::new(1600.0, 80.0, 1.0)).to(x),
                    move |guide, x| {
                        let x = x.clamp(0.0, width);
                        let minute = Timeline::minute_at(offset + x, hour_width);
                        let label_left = if x + 52.0 <= width { x + 7.0 } else { (x - 51.0).max(0.0) };
                        guide.child(div().absolute().left(px(x)).top_0().bottom_0()
                            .w(px(1.0)).bg(rgb(0x888888)))
                            .child(div().absolute().left(px(label_left)).bottom_0()
                                .text_size(px(16.0)).line_height(relative(1.0)).whitespace_nowrap()
                                .child(format!("{:02}:{:02}", minute / 60, minute % 60)))
                    },
                ))
            })
            .when_some(marker_x, |d, x| {
                d.child(div().absolute().left(px(x)).top_0().bottom_0()
                    .w(px(3.0)).bg(rgb(0xf90000)))
            })
            .child(div().id("zoom-controls").absolute().right(px(22.0)).bottom(px(15.0))
                .flex().items_center().p(px(4.0)).rounded_full()
                .border_1().border_color(rgb(LINE)).bg(rgb(0xffffff)).cursor_default()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_drop(|_: &ActivityDrag, _, _| {})
                .child(icon_button("zoom-in", "Zoom in (+)", IconName::ZoomIn, 28.0)
                    .hover(|s| s.bg(rgb(0xf0f0f0)))
                    .disabled(zoom >= 200).when(zoom >= 200, |b| b.text_color(rgb(0xaaaaaa)))
                    .on_click(cx.listener(|this, _, window, cx| this.zoom(this.timeline.zoom + 25, window, cx))))
                .child(button("zoom-reset", "Reset zoom (0)").w(px(42.0)).h(px(28.0)).rounded_full()
                    .hover(|s| s.bg(rgb(0xf0f0f0)))
                    .text_size(px(13.0)).child(format!("{zoom}%"))
                    .on_click(cx.listener(|this, _, window, cx| this.zoom(100, window, cx))))
                .child(icon_button("zoom-out", "Zoom out (−)", IconName::ZoomOut, 28.0)
                    .hover(|s| s.bg(rgb(0xf0f0f0)))
                    .disabled(zoom <= 50).when(zoom <= 50, |b| b.text_color(rgb(0xaaaaaa)))
                    .on_click(cx.listener(|this, _, window, cx| this.zoom(this.timeline.zoom - 25, window, cx)))))
    }
}

impl Render for Qalendar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = f32::from(window.viewport_size().width);
        let main_width = Self::main_width(window);
        let sidebar_width = width - main_width;
        let now = Instant::now();
        let reduced = cx.reduce_motion();
        let month_slide = self.month_transition.update(self.selected, now, reduced);
        let view_target = f32::from(self.view == View::Time);
        let view_progress =
            self.view_motion
                .update(view_target, now, Duration::from_millis(180), reduced);
        if month_slide.is_some() || view_progress != view_target {
            window.request_animation_frame();
        }
        let view_offset = view_target - view_progress;
        let page_opacity = ease_out_quint()(1.0 - view_offset.abs());
        div()
            .id("calendar-app")
            .relative()
            .track_focus(&self.focus)
            .tab_group()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .text_color(rgb(INK))
            .font_family(FONT)
            .overflow_hidden()
            .on_key_down(cx.listener(Self::key_down))
            .child(self.top_bar())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(main_width))
                            .h_full()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(self.header(month_slide, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .relative()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_h_0()
                                            .left(px(view_offset.signum()
                                                * 16.0
                                                * (1.0 - page_opacity)))
                                            .opacity(page_opacity)
                                            .child(match self.view {
                                                View::Calendar => self
                                                    .calendar(
                                                        main_width,
                                                        f32::from(window.viewport_size().height),
                                                        month_slide,
                                                        cx,
                                                    )
                                                    .into_any_element(),
                                                View::Time => self
                                                    .time_view(main_width, window, cx)
                                                    .into_any_element(),
                                            }),
                                    ),
                            ),
                    )
                    .child(self.activity_sidebar(sidebar_width, window, cx)),
            )
    }
}

#[cfg(target_os = "windows")]
fn sync_reduced_motion(cx: &mut App) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };

    let mut animations_enabled = 1i32;
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut animations_enabled as *mut i32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if result.is_ok() {
        cx.set_reduce_motion(animations_enabled == 0);
    }
}

fn main() {
    application().with_assets(assets::AllAssets).run(|cx| {
        #[cfg(target_os = "windows")]
        sync_reduced_motion(cx);
        cx.text_system()
            .add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Archivo-Regular.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Archivo-SemiBold.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Archivo-Bold.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Archivo-Black.ttf")),
            ])
            .expect("Failed to load the bundled Archivo fonts");
        init(cx);
        Theme::change(ThemeMode::Light, None, cx);
        Theme::update(cx, |theme| theme.font_family = FONT.into());
        let work_area = cx
            .primary_display()
            .map(|display| display.visible_bounds())
            .unwrap_or(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(1440.0), px(1024.0)),
            ));
        let initial_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
        let bounds = Bounds::new(
            work_area.center() - (initial_size / 2.0).into(),
            initial_size,
        );
        open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(initial_size),
                titlebar: Some(TitlebarOptions {
                    title: Some("qalendar".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                app_owns_titlebar_drag: true,
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx: &mut Context<Qalendar>| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    cx.spawn(async move |this, cx| {
                        loop {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(1))
                                .await;
                            if this
                                .update(cx, |_, cx| {
                                    #[cfg(target_os = "windows")]
                                    sync_reduced_motion(cx);
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    })
                    .detach();
                    let storage_path = std::env::var_os("QALENDAR_DATA_PATH")
                        .or_else(|| std::env::var_os("CURATED_DATA_PATH"))
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| {
                            let root = std::env::var_os("LOCALAPPDATA")
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| std::path::PathBuf::from("."));
                            activity::default_storage_path(&root)
                        });
                    let loaded = activity::load(&storage_path);
                    let storage_error = loaded.as_ref().err().cloned();
                    let load_failed = loaded.is_err();
                    let activities = loaded.unwrap_or_default();
                    let last_activity_color =
                        activities.iter().max_by_key(|a| a.id).map(|a| a.color);
                    let selected = Local::now().date_naive();
                    let mut app = Qalendar {
                        selected,
                        view: View::Calendar,
                        hovered_view: None,
                        month_transition: MonthTransition {
                            date: selected,
                            outgoing: None,
                        },
                        view_motion: ActivityMotion::new(0.0),
                        timeline: Timeline::default(),
                        wheel_scroll: None,
                        lane_scroll: 0.0,
                        cursor_x: None,
                        drag_x: None,
                        focus,
                        date_focus: cx.focus_handle(),
                        timeline_focus: cx.focus_handle(),
                        activities,
                        last_activity_color,
                        activity_tab: ActivityTab::Day,
                        hovered_activity_tab: None,
                        activity_tab_motion: ActivityMotion::new(1.0),
                        activity_form_motion: ActivityMotion::new(0.0),
                        day_odometer: DayOdometer::new(selected.day()),
                        draft: None,
                        exiting_draft: None,
                        form_open: false,
                        preview: None,
                        storage_path,
                        storage_error,
                        load_failed,
                    };
                    app.reset(window, cx);
                    app
                })
            },
        )
        .expect("Failed to open qalendar");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
