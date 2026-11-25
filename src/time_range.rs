use crate::{activity::parse_time, button, icon, *};
use chrono::{Local, NaiveTime, TimeDelta};
use gpui_kit::base::{Popover, actions::Confirm, input::MoveDown};
use gpui_kit::component::{
    ActiveTheme as _, ThemeStyled as _,
    button::{Button as StyledButton, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
};
use std::{borrow::Cow, cell::Cell, rc::Rc, time::Duration};

fn time_entry(value: &str, blank: Option<NaiveTime>) -> Result<Option<NaiveTime>, String> {
    if value.trim().is_empty() {
        Ok(blank)
    } else {
        parse_time(value).map(Some)
    }
}

fn complete_time_entry(value: &str) -> bool {
    let value = value.trim();
    if let Some((hours, minutes)) = value.split_once(':') {
        (1..=2).contains(&hours.len())
            && minutes.len() == 2
            && hours
                .bytes()
                .chain(minutes.bytes())
                .all(|byte| byte.is_ascii_digit())
    } else {
        value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit())
    }
}

fn valid_time_input(value: &str) -> bool {
    let (hours, minutes) = value.split_once(':').unwrap_or((value, ""));
    hours
        .bytes()
        .chain(minutes.bytes())
        .all(|byte| byte.is_ascii_digit())
        && limit_time_digits(value) == value
}

fn limit_time_digits(value: &str) -> Cow<'_, str> {
    let time = value.trim();
    if let Some((hours, minutes)) = time.split_once(':') {
        if (hours.len() > 2 || minutes.len() > 2)
            && hours
                .bytes()
                .chain(minutes.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return format!(
                "{}:{}",
                &hours[..hours.len().min(2)],
                &minutes[..minutes.len().min(2)]
            )
            .into();
        }
    } else if time.len() > 4 && time.bytes().all(|byte| byte.is_ascii_digit()) {
        return Cow::Borrowed(&time[..4]);
    }
    Cow::Borrowed(value)
}

fn time_input(input: &Entity<InputState>) -> Input {
    let state = input.clone();
    Input::new(input).on_paste(move |clipboard, window, cx| {
        let Some(text) = clipboard.text() else {
            return false;
        };
        let input = state.read(cx);
        let mut candidate = input.value().to_string();
        candidate.replace_range(input.selected_range(), &text);
        let limited = limit_time_digits(&candidate);
        if limited == candidate.as_str() {
            return false;
        }
        state.update(cx, |input, cx| {
            input.replace_all(limited.into_owned(), window, cx)
        });
        true
    })
}

fn error_indicator(id: &'static str, message: &'static str, color: Hsla) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .child(icon(IconName::CircleAlert, 12.0).text_color(color))
        .tooltip(move |window, cx| Tooltip::new(message).build(window, cx))
}

fn format_time_entry(
    input: &Entity<InputState>,
    blank: Option<NaiveTime>,
    window: &mut Window,
    cx: &mut App,
) -> Result<Option<NaiveTime>, String> {
    let value = input.read(cx).value();
    let time = time_entry(&value, blank)?;
    let formatted = time
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default();
    if value.as_ref() != formatted {
        input.update(cx, |state, cx| state.replace_all(formatted, window, cx));
    }
    Ok(time)
}

fn time_errors(start: &str, end: &str) -> (Option<&'static str>, Option<&'static str>) {
    let start = time_entry(start, Some(NaiveTime::MIN));
    let end = time_entry(end, None);
    let start_error = start
        .is_err()
        .then_some("Enter Start as a time such as 09:30.");
    let end_error = match (start, end) {
        (_, Err(_)) => Some("Enter End as a time such as 09:30, or clear it."),
        (Ok(Some(start)), Ok(Some(end))) if end < start => {
            Some("End time cannot be earlier than Start.")
        }
        _ => None,
    };
    (start_error, end_error)
}

pub struct TimeRange {
    pub start: Entity<InputState>,
    pub end: Entity<InputState>,
    end_options_focus: FocusHandle,
    end_menu_open: Rc<Cell<bool>>,
    _subscriptions: Vec<Subscription>,
}

impl TimeRange {
    pub fn new(
        start: Option<NaiveTime>,
        end: Option<NaiveTime>,
        window: &mut Window,
        cx: &mut Context<Qalendar>,
    ) -> Self {
        let start = cx.new(|cx| {
            InputState::new(window, cx)
                .validate(|value, _| valid_time_input(value))
                .placeholder("hh:mm")
                .default_value(
                    start
                        .unwrap_or_else(|| Local::now().time())
                        .format("%H:%M")
                        .to_string(),
                )
        });
        let end = cx.new(|cx| {
            InputState::new(window, cx)
                .validate(|value, _| valid_time_input(value))
                .placeholder("hh:mm")
                .default_value(
                    end.map(|t| t.format("%H:%M").to_string())
                        .unwrap_or_default(),
                )
        });
        let subscriptions = [(&start, Some(NaiveTime::MIN)), (&end, None)]
            .into_iter()
            .map(|(input, blank)| {
                let mut previous_len = input.read(cx).value().len();
                cx.subscribe_in(input, window, move |_, input, event, window, cx| {
                    let value = input.read(cx).value();
                    // Leave deletions and undo alone so the separator can be edited normally.
                    let grew_or_replaced = value.len() >= previous_len;
                    previous_len = value.len();
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                        || (matches!(event, InputEvent::Change)
                            && grew_or_replaced
                            && complete_time_entry(&value))
                    {
                        let _ = format_time_entry(input, blank, window, cx);
                    }
                    cx.notify();
                })
            })
            .collect();
        Self {
            start,
            end,
            end_options_focus: cx.focus_handle(),
            end_menu_open: Rc::new(Cell::new(false)),
            _subscriptions: subscriptions,
        }
    }
    pub fn value(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(NaiveTime, Option<NaiveTime>), String> {
        let start = format_time_entry(&self.start, Some(NaiveTime::MIN), window, cx)?;
        let end = format_time_entry(&self.end, None, window, cx)?;
        Ok((start.unwrap_or(NaiveTime::MIN), end))
    }
    pub fn errors(&self, cx: &App) -> (Option<&'static str>, Option<&'static str>) {
        time_errors(&self.start.read(cx).value(), &self.end.read(cx).value())
    }
    pub fn label(&self, cx: &App) -> String {
        let start = self.start.read(cx).value();
        let end = self.end.read(cx).value();
        let start = if start.trim().is_empty() {
            "00:00"
        } else {
            &start
        };
        if end.trim().is_empty() {
            start.to_string()
        } else {
            format!("{start} → {end}")
        }
    }
}

impl Qalendar {
    pub fn time_range(draft: &Draft, editing: bool, cx: &App) -> AnyElement {
        let range = &draft.time;
        let theme = cx.theme();
        let (start_error, end_error) = range.errors(cx);
        let error = start_error.or(end_error).filter(|_| draft.timed);
        let accessible_label = error.map_or_else(
            || "Choose time range".into(),
            |message| format!("Choose time range. {message}"),
        );
        let trigger = button("time-range", accessible_label)
            .w_full()
            .h(px(34.0))
            .px(px(12.0))
            .justify_between()
            .rounded(px(12.0))
            .border_color(theme.input)
            .when(error.is_some(), |trigger| {
                trigger.border_color(theme.danger)
            })
            .bg(theme.input_background())
            .text_size(px(13.0))
            .disabled(!editing || !draft.timed)
            .when(!draft.timed, |b| b.opacity(0.5))
            .child(range.label(cx))
            .child(match error {
                Some(message) => {
                    error_indicator("time-trigger-error", message, theme.danger).into_any_element()
                }
                None => icon(IconName::Clock, 14.0).into_any_element(),
            });
        if !editing || !draft.timed {
            return trigger.into_any_element();
        }
        let start = range.start.clone();
        let end = range.end.clone();
        let end_options_focus = range.end_options_focus.clone();
        let end_menu_open = range.end_menu_open.clone();
        Popover::new("time-range-popover")
            .w_full()
            .anchor(Anchor::TopLeft)
            .offset(px(6.0))
            .trigger_with(|open, _, _| trigger.aria_expanded(open).into_any_element())
            .overlay_closable(false)
            .on_open_change({
                let end_menu_open = end_menu_open.clone();
                move |_, _, _| end_menu_open.set(false)
            })
            .content(move |_, window, cx| {
                let theme = cx.theme();
                let end_value = end.read(cx).value();
                let end_focus = end.focus_handle(cx);
                let start_entry = time_entry(&start.read(cx).value(), Some(NaiveTime::MIN));
                let end_entry = time_entry(&end_value, None);
                let start_time = start_entry.ok().flatten();
                let current_end = end_entry.ok().flatten();

                let end_menu = StyledButton::new("activity-end-options")
                    .ghost()
                    .xsmall()
                    .compact()
                    .size(px(18.0))
                    .track_focus(&end_options_focus)
                    .accessibility_label("Show end time suggestions")
                    .child(icon(IconName::ChevronDown, 12.0))
                    .dropdown_menu_with_anchor(Anchor::TopRight, {
                        let end = end.clone();
                        move |menu, _, _| {
                            let mut options = vec![("No end time".to_string(), None)];
                            if let Some(start) = start_time {
                                options.extend(
                                    [(30, "30 min"), (60, "1 hr"), (90, "1 hr 30 min")]
                                        .into_iter()
                                        .filter_map(|(minutes, duration)| {
                                            let time = start + TimeDelta::minutes(minutes);
                                            (time >= start).then(|| {
                                                (
                                                    format!(
                                                        "{} · {duration}",
                                                        time.format("%H:%M")
                                                    ),
                                                    Some(time),
                                                )
                                            })
                                        }),
                                );
                            }

                            options.into_iter().fold(
                                menu.min_w(px(164.0)),
                                |menu, (label, time)| {
                                    let end = end.clone();
                                    menu.item(
                                        PopupMenuItem::new(label)
                                            .checked(if time.is_some() {
                                                time == current_end
                                            } else {
                                                end_value.trim().is_empty()
                                            })
                                            .on_click(move |_, window, cx| {
                                                let value = time
                                                    .map(|time| time.format("%H:%M").to_string())
                                                    .unwrap_or_default();
                                                end.update(cx, |input, cx| {
                                                    input.replace_all(value, window, cx)
                                                });
                                            }),
                                    )
                                },
                            )
                        }
                    })
                    .on_open_change({
                        let end_menu_open = end_menu_open.clone();
                        move |open, _, _| end_menu_open.set(*open)
                    });
                let end_field = div()
                    .id("activity-end-field")
                    .capture_action(move |_: &MoveDown, window, cx| {
                        if end_focus.is_focused(window) {
                            cx.stop_propagation();
                            end_options_focus.dispatch_action(
                                &Confirm { secondary: false },
                                window,
                                cx,
                            );
                        }
                    })
                    .child(
                        time_input(&end)
                            .aria_label(end_error.unwrap_or("End time, 24-hour HH:MM"))
                            .small()
                            .h(px(30.0))
                            .pl(px(6.0))
                            .gap(px(2.0))
                            .when(end_error.is_some(), |field| {
                                field.border_color(theme.danger)
                            })
                            .suffix(
                                div()
                                    .flex()
                                    .items_center()
                                    .when_some(end_error, |suffix, message| {
                                        suffix.child(error_indicator(
                                            "end-time-error",
                                            message,
                                            theme.danger,
                                        ))
                                    })
                                    .child(end_menu),
                            ),
                    );

                let panel = div()
                    .id("time-range-panel")
                    // Match the Date picker: 224px content with 12px padding on each side.
                    .w(px(224.0) + window.rem_size() * 1.5)
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .p_3()
                    .popover_style(cx)
                    .on_mouse_down_out(cx.listener(move |state, _, window, cx| {
                        // The dropdown owns outside clicks while it sits above this panel.
                        if !end_menu_open.get() {
                            state.dismiss(window, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(4.0))
                                    .child("Start")
                                    .child(
                                        time_input(&start)
                                            .aria_label(
                                                start_error.unwrap_or("Start time, 24-hour HH:MM"),
                                            )
                                            .small()
                                            .when_some(start_error, |field, message| {
                                                field.border_color(theme.danger).suffix(
                                                    error_indicator(
                                                        "start-time-error",
                                                        message,
                                                        theme.danger,
                                                    ),
                                                )
                                            })
                                            .h(px(30.0)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(4.0))
                                    .child("End")
                                    .child(end_field),
                            ),
                    );

                animate_picker("time-popover-enter", panel)
            })
            .into_any_element()
    }
}

pub(crate) fn animate_picker<T: IntoElement + Styled + 'static>(
    id: &'static str,
    mut panel: T,
) -> impl IntoElement {
    let shadow = panel.style().box_shadow.clone().unwrap_or_default();
    panel.with_animation(
        id,
        Animation::new(Duration::from_millis(150))
            .with_easing(|progress| 1.0 - (1.0 - progress).powi(3)),
        move |panel, progress| {
            let shadow_strength = progress.powi(3);
            panel
                .top(px(-8.0 * (1.0 - progress)))
                .opacity(progress)
                .shadow(
                    shadow
                        .iter()
                        .cloned()
                        .map(|mut layer| {
                            layer.color.a *= shadow_strength;
                            layer
                        })
                        .collect(),
                )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        NaiveTime, complete_time_entry, limit_time_digits, parse_time, time_entry, time_errors,
        valid_time_input,
    };

    #[test]
    fn earlier_end_sets_only_the_end_field_error() {
        assert_eq!(
            time_errors("06:48", "04:00"),
            (None, Some("End time cannot be earlier than Start."))
        );
        for end in ["", "06:48", "07:00"] {
            assert_eq!(time_errors("06:48", end), (None, None));
        }
    }

    #[test]
    fn time_fields_accept_partial_times_and_reject_invalid_characters() {
        for value in ["", "2", "2322", "09:", ":30", "09:30"] {
            assert!(valid_time_input(value), "{value}");
        }
        for value in ["2aseaa", "09:ab", "09::30", "09 30", "11:1111"] {
            assert!(!valid_time_input(value), "{value}");
        }
    }

    #[test]
    fn caps_extra_time_digits_without_changing_partial_entries() {
        for (entry, expected) in [
            ("11:1111", "11:11"),
            ("123:456", "12:45"),
            ("223211", "2232"),
            ("9:301", "9:30"),
            ("930", "930"),
            ("9:", "9:"),
            ("", ""),
            ("9:💥", "9:💥"),
        ] {
            assert_eq!(limit_time_digits(entry), expected);
        }
    }

    #[test]
    fn time_entries_accept_shorthand_and_use_field_specific_empty_values() {
        for entry in ["930", "0930", "9:30", "09:30"] {
            assert_eq!(
                parse_time(entry).unwrap().format("%H:%M").to_string(),
                "09:30"
            );
        }
        assert_eq!(
            parse_time("9").unwrap().format("%H:%M").to_string(),
            "09:00"
        );
        assert_eq!(
            time_entry("", Some(NaiveTime::MIN)).unwrap(),
            Some(NaiveTime::MIN)
        );
        assert_eq!(time_entry(" ", None).unwrap(), None);
        assert_eq!(
            time_entry("24:00", Some(NaiveTime::MIN)).unwrap(),
            Some(NaiveTime::MIN)
        );
        assert!(time_entry("9:x", Some(NaiveTime::MIN)).is_err());
    }

    #[test]
    fn full_times_format_live_without_consuming_partial_entries() {
        for (entry, expected) in [
            ("2232", "22:32"),
            ("2322", "23:22"),
            ("2400", "00:00"),
            ("25:15", "01:15"),
            ("2360", "00:00"),
            ("12:75", "13:15"),
            ("9:30", "09:30"),
        ] {
            assert!(complete_time_entry(entry));
            assert_eq!(
                parse_time(entry).unwrap().format("%H:%M").to_string(),
                expected
            );
        }
        for entry in ["", "9", "93", "930", "9:", "09:3", "12x0"] {
            assert!(!complete_time_entry(entry));
        }
    }
}
