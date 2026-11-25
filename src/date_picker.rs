use crate::{time_range::animate_picker, *};
use gpui_kit::{
    base::{
        Calendar, CalendarEvent, CalendarItemKind, CalendarState, Date, Popover, input::Delete,
    },
    component::{ActiveTheme as _, ThemeStyled as _},
};

pub struct ActivityDatePicker {
    calendar: Entity<CalendarState>,
    date: Option<NaiveDate>,
    open: bool,
    focus: FocusHandle,
    _selection: Subscription,
}

impl ActivityDatePicker {
    pub fn new(date: NaiveDate, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let calendar = cx.new(|cx| {
            let mut state = CalendarState::new(window, cx);
            state.set_date(date, window, cx);
            state
        });
        let selection = cx.subscribe_in(&calendar, window, |this, _, event, window, cx| {
            if let CalendarEvent::Selected(Date::Single(Some(date))) = *event {
                this.date = Some(date);
                this.open = false;
                this.focus.focus(window, cx);
                cx.notify();
            }
        });
        Self {
            calendar,
            date: Some(date),
            open: false,
            focus: cx.focus_handle(),
            _selection: selection,
        }
    }

    pub fn date(&self) -> Option<NaiveDate> {
        self.date
    }

    pub fn set_date(
        &mut self,
        date: Option<NaiveDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.date = date;
        self.open = false;
        self.calendar.update(cx, |state, cx| {
            state.set_date(Date::Single(date), window, cx)
        });
        cx.notify();
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    pub(crate) fn trigger(&self, cx: &App) -> Button {
        let theme = cx.theme();
        let display = self
            .date
            .map(|date| date.format("%d/%m/%y").to_string())
            .unwrap_or_else(|| "Select date".into());
        button("activity-date-field", "Activity date")
            .role(Role::ComboBox)
            .aria_value(display.clone())
            .w_full()
            .h(px(34.0))
            .px(px(10.0))
            .rounded(px(12.0))
            .justify_between()
            .border_color(theme.input)
            .bg(theme.input_background())
            .text_sm()
            .child(display)
            .child(icon(IconName::Calendar, 12.0).text_color(theme.muted_foreground))
    }
}

fn date_calendar(state: &Entity<CalendarState>) -> impl IntoElement + use<> {
    Calendar::new(("activity-date-calendar", state.entity_id()), state)
        .w(px(224.0))
        .gap_0p5()
        .label(|kind, value| match kind {
            CalendarItemKind::Weekday => {
                ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][value as usize].into()
            }
            CalendarItemKind::Month | CalendarItemKind::MonthToggle => {
                NaiveDate::from_ymd_opt(2000, value as u32, 1)
                    .map(|month| month.format("%B").to_string().into())
                    .unwrap_or_default()
            }
            _ => value.to_string().into(),
        })
        .item(|item, state, _, cx| {
            let theme = cx.theme();
            let kind = state.kind();
            item.when(
                matches!(kind, CalendarItemKind::Previous | CalendarItemKind::Next),
                |item| {
                    item.clear_children().child(icon(
                        if kind == CalendarItemKind::Previous {
                            IconName::ChevronLeft
                        } else {
                            IconName::ChevronRight
                        },
                        16.0,
                    ))
                },
            )
            .size_8()
            .rounded(theme.radius)
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .when(
                matches!(
                    kind,
                    CalendarItemKind::MonthToggle | CalendarItemKind::YearToggle
                ),
                |item| item.w_auto().px_2().font_weight(FontWeight::MEDIUM),
            )
            .when(
                matches!(kind, CalendarItemKind::Month | CalendarItemKind::Year),
                |item| item.w_full().my_1(),
            )
            .when(
                matches!(kind, CalendarItemKind::Weekday | CalendarItemKind::Month),
                |item| item.text_xs(),
            )
            .when(state.is_muted(), |item| {
                item.text_color(theme.muted_foreground)
                    .when(state.is_disabled(), |item| item.opacity(0.5))
            })
            .when(!state.is_active() && !state.is_disabled(), |item| {
                item.hover(|item| {
                    item.bg(theme.tokens.secondary_hover)
                        .text_color(theme.foreground)
                })
            })
            .when(state.is_today(), |item| {
                item.bg(theme.accent).text_color(theme.accent_foreground)
            })
            .when(state.is_active(), |item| {
                item.bg(theme.tokens.primary)
                    .text_color(theme.primary_foreground)
            })
            .when(kind == CalendarItemKind::Day && state.is_active(), |item| {
                item.bg(rgb(BLUE)).text_color(rgb(0xffffff))
            })
            .into_any_element()
        })
}

impl Render for ActivityDatePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let calendar = self.calendar.clone();
        let view = cx.entity().downgrade();
        let trigger = self
            .trigger(cx)
            .when(self.open || self.focus.is_focused(window), |trigger| {
                trigger.border_color(rgb(BLUE))
            })
            .key_context("DatePicker")
            .on_action(cx.listener(|this, _: &Delete, window, cx| {
                this.set_date(None, window, cx);
            }))
            .track_focus(&self.focus);

        Popover::new("activity-date-picker")
            .w_full()
            .offset(px(6.0))
            .open(self.open)
            .on_open_change(move |open, _, cx| {
                _ = view.update(cx, |this, cx| {
                    this.open = *open;
                    cx.notify();
                });
            })
            .trigger_with(move |open, _, _| trigger.aria_expanded(open).into_any_element())
            .content(move |_, _, cx| {
                animate_picker(
                    "activity-date-enter",
                    div()
                        .p_3()
                        .popover_style(cx)
                        .child(date_calendar(&calendar)),
                )
            })
    }
}
