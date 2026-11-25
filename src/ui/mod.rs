//! Building blocks shared by all panels.
//!
//! [`Ui`] carries what a view needs to know about its surroundings: the theme,
//! the width it has, and the preferences. It is `Copy` and handed down by value.
//! Panels compose its methods and contain no styling of their own, which keeps
//! thirty panels looking like one program and makes themes apply everywhere.

pub mod chart;
pub mod color;

use iced::font::Weight;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{
    Space, button, column, container, mouse_area, pick_list, row, scrollable, slider, space, svg,
    text, text_input, toggler,
};
use iced::{Alignment, Center, Color, Fill, Length, Padding, Shrink};

use crate::config::Config;
use crate::icon::Icon;
use crate::theme::style::{self, Tone};
use crate::theme::{Library, Metrics, Theme};

pub type Element<'a, M> = iced::Element<'a, M, Theme>;
pub type Text<'a> = iced::widget::Text<'a, Theme>;

/// How much room the window has. Tiling makes every one of these common.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Width {
    /// A third of a laptop screen, or a phone-sized column.
    Narrow,
    /// Half a laptop screen.
    Medium,
    /// A full tile.
    Wide,
}

impl Width {
    pub fn of(window: f32) -> Self {
        match window {
            w if w < 620.0 => Width::Narrow,
            w if w < 1100.0 => Width::Medium,
            _ => Width::Wide,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Danger,
    Link,
}

#[derive(Clone, Copy)]
pub struct Ui<'a> {
    pub theme: &'a Theme,
    pub config: &'a Config,
    pub library: &'a Library,
    pub width: Width,
    /// Room for content, inside the padding.
    pub content: f32,
}

impl<'a> Ui<'a> {
    pub fn metrics(self) -> &'a Metrics {
        self.theme.metrics()
    }

    /// Scales a length by the theme's density.
    pub fn d(self, length: f32) -> f32 {
        length * self.metrics().density
    }

    fn size(self, offset: f32) -> f32 {
        self.metrics().text_size + offset
    }

    fn styled<'b>(self, content: impl IntoFragment<'b>, offset: f32, weight: Weight) -> Text<'b> {
        text(content).size(self.size(offset)).font(self.theme.font(weight))
    }

    pub fn text<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, 0.0, Weight::Normal)
    }

    pub fn strong<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, 0.0, Weight::Medium)
    }

    pub fn bold<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, 0.0, Weight::Semibold)
    }

    pub fn dim<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.text(content).style(style::ink(Tone::Dim))
    }

    pub fn small<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, -1.5, Weight::Normal).style(style::ink(Tone::Mute))
    }

    pub fn caption<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, -1.0, Weight::Semibold).style(style::ink(Tone::Dim))
    }

    pub fn heading<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, 3.0, Weight::Semibold)
    }

    /// A reading meant to be seen from a distance, such as the battery level.
    pub fn figure<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        self.styled(content, 30.0, Weight::Bold)
    }

    /// A large percentage. The sign is set smaller and raised to the baseline of
    /// the number, as text boxes of different sizes align by their edges.
    pub fn percentage<M: 'a>(self, value: f32) -> Element<'a, M> {
        let raise = Padding::ZERO.bottom(self.size(30.0) * 0.2);
        row![self.figure(format!("{value:.0}")), container(self.heading("%")).padding(raise)]
            .spacing(1)
            .align_y(iced::Bottom)
            .into()
    }

    pub fn mono<'b>(self, content: impl IntoFragment<'b>) -> Text<'b> {
        text(content).size(self.size(-1.5)).font(self.theme.mono())
    }

    pub fn icon<M: 'a>(self, icon: Icon, tone: Tone) -> Element<'a, M> {
        self.sized_icon(icon, tone, self.metrics().icon_size)
    }

    pub fn sized_icon<M: 'a>(self, icon: Icon, tone: Tone, size: f32) -> Element<'a, M> {
        svg(self.theme.icon(icon)).width(size).height(size).style(style::icon(tone)).into()
    }

    /// The frame of a panel: heading, optional action, then the body.
    pub fn panel<M: 'a>(
        self,
        title: impl IntoFragment<'a>,
        subtitle: impl IntoFragment<'a>,
        action: Option<Element<'a, M>>,
        body: Vec<Element<'a, M>>,
    ) -> Element<'a, M> {
        let grow = if self.width == Width::Narrow { 10.0 } else { 14.0 };
        let subtitle = subtitle.into_fragment();
        let mut heading = column![self.styled(title, grow, Weight::Bold)].spacing(4).width(Fill);
        if !subtitle.is_empty() {
            heading = heading.push(self.dim(subtitle));
        }
        // An action next to a long heading would squeeze it into a sliver.
        let header: Element<'a, M> = match action {
            Some(action) if self.content < 520.0 => column![heading, action].spacing(12).into(),
            Some(action) => row![heading, action].spacing(24).align_y(Center).into(),
            None => heading.into(),
        };
        let content =
            column![header].extend(body).spacing(self.metrics().gap).padding(self.padding());
        scrollable(content)
            .width(Fill)
            .height(Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6).margin(3),
            ))
            .into()
    }

    fn padding(self) -> Padding {
        let side = Self::side_padding(self.metrics(), self.width);
        Padding { top: side * 0.83, right: side, bottom: side * 0.75, left: side }
    }

    /// Horizontal padding. Small windows cannot afford the theme's full value.
    pub fn side_padding(metrics: &Metrics, width: Width) -> f32 {
        match width {
            Width::Wide => metrics.content_padding,
            Width::Medium => metrics.content_padding.min(28.0),
            Width::Narrow => metrics.content_padding.min(16.0),
        }
    }

    /// A titled card of rows separated by hairlines.
    pub fn group<M: 'a>(
        self,
        title: impl IntoFragment<'a>,
        rows: Vec<Element<'a, M>>,
    ) -> Element<'a, M> {
        let title = title.into_fragment();
        let mut content = column![].width(Fill);
        if !title.is_empty() {
            let heading = container(self.caption(title)).padding(Padding {
                top: self.d(15.0),
                right: 18.0,
                bottom: self.d(7.0),
                left: 18.0,
            });
            content = content.push(heading);
        }
        for (index, row) in rows.into_iter().enumerate() {
            if index > 0 {
                content = content.push(self.divider());
            }
            content = content.push(row);
        }
        container(content).style(style::card).width(Fill).clip(true).into()
    }

    /// A card with free-form content.
    pub fn card<M: 'a>(self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        container(content).padding(18).style(style::card).width(Fill).into()
    }

    pub fn divider<M: 'a>(self) -> Element<'a, M> {
        container(space()).width(Fill).height(1).style(style::divider).into()
    }

    fn labels<M: 'a>(
        self,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
    ) -> Element<'a, M> {
        let hint = hint.into_fragment();
        let mut labels = column![self.text(label)].spacing(1).width(Fill);
        if !hint.is_empty() {
            labels = labels.push(self.small(hint));
        }
        labels.into()
    }

    /// Pads a row and gives it the minimum height that keeps it easy to hit.
    fn frame<M: 'a>(self, content: iced::widget::Row<'a, M, Theme>) -> Element<'a, M> {
        let strut = Space::new().height(self.d(38.0));
        container(row![strut, content.align_y(Center).width(Fill)].align_y(Center))
            .padding([self.d(8.0), 18.0])
            .width(Fill)
            .into()
    }

    /// A label with a control on the right.
    pub fn row<M: 'a>(
        self,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        control: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        self.frame(row![self.labels(label, hint), control.into()].spacing(16))
    }

    pub fn icon_row<M: 'a>(
        self,
        icon: Icon,
        tone: Tone,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        control: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        let icon = self.icon(icon, tone);
        self.frame(row![icon, self.labels(label, hint), control.into()].spacing(14))
    }

    /// A label with a plain value.
    pub fn value<M: 'a>(
        self,
        label: impl IntoFragment<'a>,
        value: impl IntoFragment<'a>,
    ) -> Element<'a, M> {
        let value = self.dim(value).width(Fill).align_x(Alignment::End);
        self.frame(row![self.text(label), value].spacing(16))
    }

    /// Free-form content with the padding of a row.
    pub fn cell<M: 'a>(self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        container(content).padding([self.d(10.0), 18.0]).width(Fill).into()
    }

    /// A switch. `None` draws it disabled, for hardware that lacks the feature.
    pub fn toggle<M: Clone + 'a>(
        self,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        value: bool,
        on_toggle: Option<impl Fn(bool) -> M + 'a>,
    ) -> Element<'a, M> {
        self.row(label, hint, self.switch(value, on_toggle))
    }

    pub fn switch<M: Clone + 'a>(
        self,
        value: bool,
        on_toggle: Option<impl Fn(bool) -> M + 'a>,
    ) -> Element<'a, M> {
        toggler(value).size(24).on_toggle_maybe(on_toggle).into()
    }

    /// A slider row. `on_change` fires while dragging and `on_release` once.
    #[allow(clippy::too_many_arguments)]
    pub fn slide<M: Clone + 'a>(
        self,
        label: impl IntoFragment<'a>,
        range: std::ops::RangeInclusive<f32>,
        step: f32,
        value: f32,
        shown: impl IntoFragment<'a>,
        on_change: impl Fn(f32) -> M + 'a,
        on_release: M,
    ) -> Element<'a, M> {
        let label_width = if self.width == Width::Narrow { 104.0 } else { 136.0 };
        self.frame(
            row![
                self.text(label).width(label_width),
                self.slider(range, step, value, on_change, on_release),
                self.reading(shown),
            ]
            .spacing(14),
        )
    }

    pub fn icon_slide<M: Clone + 'a>(
        self,
        icon: Icon,
        percent: f32,
        on_change: impl Fn(f32) -> M + 'a,
        on_release: M,
    ) -> Element<'a, M> {
        self.frame(
            row![
                self.icon(icon, Tone::Text),
                self.slider(0.0..=100.0, 1.0, percent, on_change, on_release),
                self.reading(format!("{percent:.0}%")),
            ]
            .spacing(14),
        )
    }

    pub fn slider<M: Clone + 'a>(
        self,
        range: std::ops::RangeInclusive<f32>,
        step: f32,
        value: f32,
        on_change: impl Fn(f32) -> M + 'a,
        on_release: M,
    ) -> Element<'a, M> {
        slider(range, value, on_change).step(step).on_release(on_release).height(28).into()
    }

    fn reading(self, shown: impl IntoFragment<'a>) -> Text<'a> {
        self.dim(shown).width(56).align_x(Alignment::End).wrapping(Wrapping::None)
    }

    /// A dropdown. Long menus scroll instead of running off the window.
    pub fn pick<T, M>(
        self,
        options: Vec<T>,
        selected: Option<T>,
        on_select: impl Fn(T) -> M + 'a,
    ) -> Element<'a, M>
    where
        T: ToString + PartialEq + Clone + 'a,
        M: Clone + 'a,
    {
        // A dropdown is as wide as its widest option, which would push the label
        // out of the row when that option is a sentence.
        let longest = options.iter().map(|o| o.to_string().chars().count()).max().unwrap_or(0);
        let width = match self.width {
            Width::Narrow => Length::Fixed(160.0),
            _ if longest > 28 => Length::Fixed(240.0),
            _ => Shrink,
        };
        self.dropdown(options, selected, on_select, width)
    }

    /// A dropdown with an explicit width.
    pub fn dropdown<T, M>(
        self,
        options: Vec<T>,
        selected: Option<T>,
        on_select: impl Fn(T) -> M + 'a,
        width: Length,
    ) -> Element<'a, M>
    where
        T: ToString + PartialEq + Clone + 'a,
        M: Clone + 'a,
    {
        let height = if options.len() > 9 { Length::Fixed(320.0) } else { Shrink };
        pick_list(options, selected, on_select)
            .placeholder("Choose")
            .text_size(self.size(0.0))
            .font(self.theme.font(Weight::Normal))
            .padding([self.d(8.0), 12.0])
            .menu_height(height)
            .width(width)
            .into()
    }

    /// A dropdown over fixed `(value, label)` pairs.
    pub fn choose<M: Clone + 'a>(
        self,
        options: &[(&'static str, &'static str)],
        selected: &str,
        on_select: impl Fn(&'static str) -> M + 'a,
    ) -> Element<'a, M> {
        let options: Vec<_> =
            options.iter().map(|(value, label)| Choice { value, label }).collect();
        let selected = options.iter().copied().find(|choice| choice.value == selected);
        self.pick(options, selected, move |choice| on_select(choice.value))
    }

    /// A few exclusive options, all visible.
    pub fn segments<T, M>(
        self,
        options: impl IntoIterator<Item = (T, &'a str)>,
        selected: &T,
        on_select: impl Fn(T) -> M,
        on_page: bool,
    ) -> Element<'a, M>
    where
        T: PartialEq,
        M: Clone + 'a,
    {
        let buttons = options.into_iter().map(|(value, label)| {
            let on = value == *selected;
            button(self.strong(label).align_x(Center).wrapping(Wrapping::None))
                .padding([self.d(9.0), 13.0])
                .style(style::segment(on))
                .on_press(on_select(value))
                .into()
        });
        container(row(buttons).spacing(2))
            .padding(3)
            .style(if on_page { style::tray } else { style::well })
            .into()
    }

    /// One of several exclusive rows, with room for an explanation.
    pub fn option<M: Clone + 'a>(
        self,
        icon: Icon,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        selected: bool,
        on_press: M,
    ) -> Element<'a, M> {
        let tone = if selected { Tone::Accent } else { Tone::Mute };
        let content = row![self.icon(icon, tone), self.labels(label, hint), self.dot(selected)]
            .spacing(14)
            .align_y(Center);
        button(content)
            .padding([self.d(11.0), 18.0])
            .width(Fill)
            .style(style::row)
            .on_press(on_press)
            .into()
    }

    /// A radio indicator.
    pub fn dot<M: 'a>(self, on: bool) -> Element<'a, M> {
        container(space()).width(20).height(20).style(style::radio(on)).into()
    }

    /// A selectable card in a grid.
    pub fn pill<M: Clone + 'a>(
        self,
        label: impl IntoFragment<'a>,
        selected: bool,
        on_press: M,
    ) -> Element<'a, M> {
        let content = row![self.dot(selected), self.strong(label).wrapping(Wrapping::None)]
            .spacing(12)
            .align_y(Center);
        button(content)
            .padding([self.d(13.0), 14.0])
            .width(Fill)
            .clip(true)
            .style(style::option(selected))
            .on_press(on_press)
            .into()
    }

    /// A row that leads somewhere.
    pub fn link<M: Clone + 'a>(
        self,
        icon: Icon,
        tone: Tone,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        on_press: M,
    ) -> Element<'a, M> {
        self.leading(icon, tone, label, hint, None, on_press)
    }

    /// A link to something that needs attention.
    ///
    /// The icon says what it is about and stays in the colour of its neighbours.
    /// A dot marks the urgency, so a list of notes reads as one set.
    pub fn alert<M: Clone + 'a>(
        self,
        icon: Icon,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        on_press: M,
    ) -> Element<'a, M> {
        let dot = container(space()).width(8).height(8).style(style::meter(Some(Tone::Warning)));
        self.leading(icon, Tone::Mute, label, hint, Some(dot.into()), on_press)
    }

    fn leading<M: Clone + 'a>(
        self,
        icon: Icon,
        tone: Tone,
        label: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        mark: Option<Element<'a, M>>,
        on_press: M,
    ) -> Element<'a, M> {
        let content = row![self.icon(icon, tone), self.labels(label, hint)]
            .push(mark)
            .push(self.icon(Icon::ChevronRight, Tone::Mute))
            .spacing(14)
            .align_y(Center);
        button(content)
            .padding([self.d(12.0), 18.0])
            .width(Fill)
            .style(style::row)
            .on_press(on_press)
            .into()
    }

    pub fn button<M: Clone + 'a>(
        self,
        label: impl IntoFragment<'a>,
        kind: Kind,
        on_press: Option<M>,
    ) -> Element<'a, M> {
        let (style, padding): (fn(&Theme, button::Status) -> button::Style, _) = match kind {
            Kind::Primary => (style::primary, [self.d(11.0), 18.0]),
            Kind::Secondary => (style::secondary, [self.d(9.0), 14.0]),
            Kind::Danger => (style::danger, [self.d(9.0), 14.0]),
            Kind::Link => (style::link, [self.d(9.0), 6.0]),
        };
        button(self.bold(label).wrapping(Wrapping::None))
            .padding(padding)
            .style(style)
            .on_press_maybe(on_press)
            .into()
    }

    pub fn icon_button<M: Clone + 'a>(
        self,
        icon: Icon,
        tone: Tone,
        on_press: Option<M>,
    ) -> Element<'a, M> {
        button(self.sized_icon(icon, tone, 18.0))
            .padding(9)
            .style(style::secondary)
            .on_press_maybe(on_press)
            .into()
    }

    pub fn field<M: Clone + 'a>(
        self,
        placeholder: &str,
        value: &str,
        on_input: impl Fn(String) -> M + 'a,
    ) -> text_input::TextInput<'a, M, Theme> {
        text_input(placeholder, value)
            .on_input(on_input)
            .size(self.size(0.0))
            .font(self.theme.font(Weight::Normal))
            .padding([self.d(10.0), 12.0])
            .style(style::inset_field)
    }

    /// A search field with its icon. `hint` names a keyboard shortcut.
    pub fn search<M: Clone + 'a>(
        self,
        placeholder: &str,
        hint: &'a str,
        input: impl FnOnce(text_input::TextInput<'a, M, Theme>) -> text_input::TextInput<'a, M, Theme>,
        value: &str,
        on_input: impl Fn(String) -> M + 'a,
    ) -> Element<'a, M> {
        let field = self
            .field(placeholder, value, on_input)
            .padding(Padding { top: self.d(11.0), right: 72.0, bottom: self.d(11.0), left: 42.0 })
            .style(style::field);
        let icon = container(self.sized_icon(Icon::Search, Tone::Mute, 18.0))
            .padding(Padding::ZERO.left(14))
            .center_y(Fill);
        let hint = container(self.mono(hint).style(style::ink(Tone::Mute)))
            .padding(Padding::ZERO.right(14))
            .align_right(Fill)
            .center_y(Fill);
        let width = if self.content < 520.0 { Fill } else { Length::Fixed(340.0) };
        container(iced::widget::stack![input(field), icon, hint]).width(width).into()
    }

    /// Space-separated controls that wrap when they run out of room.
    pub fn actions<M: 'a>(self, controls: Vec<Element<'a, M>>) -> Element<'a, M> {
        row(controls).spacing(8).align_y(Center).wrap().vertical_spacing(8).into()
    }

    /// Two columns, or one where there is no room for both.
    pub fn columns<M: 'a>(
        self,
        left: impl Into<Element<'a, M>>,
        right: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        self.split(1, left, 1, right)
    }

    pub fn split<M: 'a>(
        self,
        left_share: u16,
        left: impl Into<Element<'a, M>>,
        right_share: u16,
        right: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        let gap = self.metrics().gap;
        if self.content < 760.0 {
            return column![left.into(), right.into()].spacing(gap).into();
        }
        row![
            container(left).width(Length::FillPortion(left_share)),
            container(right).width(Length::FillPortion(right_share)),
        ]
        .spacing(gap)
        .into()
    }

    pub fn stack<M: 'a>(self, children: Vec<Element<'a, M>>) -> Element<'a, M> {
        column(children).spacing(self.metrics().gap).width(Fill).into()
    }

    /// Equal columns at least `min` wide.
    ///
    /// Fewer columns than fit are used when that fills every row: eight items
    /// read better as four and four than as five and three. What is left over
    /// is padded so that items keep the same width.
    pub fn grid<M: 'a>(
        self,
        min: f32,
        available: f32,
        spacing: f32,
        items: Vec<Element<'a, M>>,
    ) -> Element<'a, M> {
        let count = column_count(min, available, spacing, items.len());
        let mut rows = column![].spacing(spacing);
        let mut items = items.into_iter().peekable();
        while items.peek().is_some() {
            let mut line = row![].spacing(spacing);
            for _ in 0..count {
                let item = items.next().unwrap_or_else(|| space().into());
                line = line.push(container(item).width(Fill));
            }
            rows = rows.push(line);
        }
        rows.into()
    }

    /// Width that [`Ui::grid`] gives each of `total` items across the content.
    pub fn column_width(self, min: f32, spacing: f32, total: usize) -> f32 {
        let count = column_count(min, self.content, spacing, total) as f32;
        (self.content - spacing * (count - 1.0)) / count
    }

    /// A list to search in, for choices too long for a dropdown.
    pub fn finder<M: Clone + 'a>(
        self,
        query: &str,
        on_query: impl Fn(String) -> M + 'a,
        items: impl Iterator<Item = (String, bool, M)>,
    ) -> Element<'a, M> {
        const LIMIT: usize = 80;
        let mut entries = column![].width(Fill);
        let (mut items, rest): (Vec<_>, Vec<_>) = items.partition(|(_, selected, _)| *selected);
        items.extend(rest);
        for (label, selected, on_press) in items.into_iter().take(LIMIT) {
            let mark: Element<'a, M> = if selected {
                self.sized_icon(Icon::Check, Tone::Accent, 16.0)
            } else {
                space().width(16).into()
            };
            let content = row![self.text(label).width(Fill), mark].align_y(Center);
            entries = entries.push(
                button(content)
                    .padding([self.d(8.0), 18.0])
                    .width(Fill)
                    .style(style::row)
                    .on_press(on_press),
            );
        }
        let search = self.field("Search", query, on_query);
        column![self.cell(search), scrollable(entries).height(260).width(Fill)].into()
    }

    pub fn badge<M: 'a>(self, label: impl IntoFragment<'a>) -> Element<'a, M> {
        container(self.styled(label, -2.0, Weight::Medium).wrapping(Wrapping::None))
            .padding([3, 8])
            .style(style::badge)
            .into()
    }

    /// A labelled bar for a share of a whole.
    pub fn meter<M: 'a>(
        self,
        label: impl IntoFragment<'a>,
        shown: impl IntoFragment<'a>,
        fraction: f32,
    ) -> Element<'a, M> {
        let tone = if fraction > 0.9 { Tone::Warning } else { Tone::Live };
        column![
            row![self.strong(label).width(Fill), self.dim(shown)].spacing(12),
            chart::bar(fraction, tone),
        ]
        .spacing(10)
        .into()
    }

    pub fn swatch<M: Clone + 'a>(self, color: Color, on_press: Option<M>) -> Element<'a, M> {
        let well = container(space()).width(32).height(32).style(style::swatch(color));
        match on_press {
            Some(message) => mouse_area(well)
                .on_press(message)
                .interaction(iced::mouse::Interaction::Pointer)
                .into(),
            None => well.into(),
        }
    }

    /// Stands in for a panel that has nothing to show, and says what to do.
    pub fn empty<M: 'a>(
        self,
        icon: Icon,
        title: impl IntoFragment<'a>,
        hint: impl IntoFragment<'a>,
        action: Option<Element<'a, M>>,
    ) -> Element<'a, M> {
        let mut content = column![
            self.sized_icon(icon, Tone::Mute, 36.0),
            self.heading(title).align_x(Center),
            self.small(hint).align_x(Center),
        ]
        .spacing(8)
        .align_x(Center)
        .max_width(420);
        if let Some(action) = action {
            content = content.push(space().height(6)).push(action);
        }
        container(content).padding([40, 20]).center_x(Fill).style(style::card).into()
    }

    /// A version and the one that takes its place.
    pub fn change<M: 'a>(self, from: &'a str, to: &'a str) -> Element<'a, M> {
        row![
            self.small(from).wrapping(Wrapping::None),
            self.sized_icon(Icon::ChevronRight, Tone::Mute, 14.0),
            self.small(to).style(style::ink(Tone::Live)).wrapping(Wrapping::None),
        ]
        .spacing(6)
        .align_y(Center)
        .into()
    }

    /// Explains a failure next to the controls it affects.
    pub fn failure<M: 'a>(self, reason: impl IntoFragment<'a>) -> Element<'a, M> {
        self.remark(Icon::Warning, Tone::Warning, reason)
    }

    /// Something to know before using the controls. The icon names the subject.
    pub fn notice<M: 'a>(self, icon: Icon, text: impl IntoFragment<'a>) -> Element<'a, M> {
        self.remark(icon, Tone::Mute, text)
    }

    fn remark<M: 'a>(self, icon: Icon, tone: Tone, text: impl IntoFragment<'a>) -> Element<'a, M> {
        let content = row![self.icon(icon, tone), self.text(text).width(Fill)];
        self.card(content.spacing(14).align_y(Center))
    }

    pub fn loading<M: 'a>(self) -> Element<'a, M> {
        self.card(self.dim("Reading from the system"))
    }
}

fn column_count(min: f32, available: f32, spacing: f32, total: usize) -> usize {
    let fit = (((available + spacing) / (min + spacing)).floor() as usize).max(1);
    (fit.div_ceil(2).max(2)..=fit)
        .rev()
        .find(|count| total > fit && total.is_multiple_of(*count))
        .unwrap_or(fit)
}

#[derive(Clone, Copy, PartialEq)]
struct Choice {
    value: &'static str,
    label: &'static str,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// Limits a stream of values to one request in flight.
///
/// Dragging a slider produces values far faster than the commands they trigger
/// can finish. Intermediate values are dropped and the latest is always sent, so
/// the system ends up where the slider does.
#[derive(Debug, Clone)]
pub struct Throttle<T> {
    busy: bool,
    pending: Option<T>,
}

impl<T> Default for Throttle<T> {
    fn default() -> Self {
        Self { busy: false, pending: None }
    }
}

impl<T> Throttle<T> {
    /// Returns the value if it can be sent now, and holds on to it otherwise.
    pub fn push(&mut self, value: T) -> Option<T> {
        if self.busy {
            self.pending = Some(value);
            return None;
        }
        self.busy = true;
        Some(value)
    }

    /// Marks the request as finished and returns the next value to send.
    pub fn done(&mut self) -> Option<T> {
        self.busy = self.pending.is_some();
        self.pending.take()
    }

    /// Drops a held value, for when a final one is about to be sent instead.
    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_keeps_only_the_latest() {
        let mut throttle = Throttle::default();
        assert_eq!(throttle.push(1), Some(1));
        assert_eq!(throttle.push(2), None);
        assert_eq!(throttle.push(3), None);
        assert_eq!(throttle.done(), Some(3));
        assert_eq!(throttle.done(), None);
        assert_eq!(throttle.push(4), Some(4));
    }

    #[test]
    fn grids_prefer_full_rows() {
        assert_eq!(column_count(172.0, 954.0, 12.0, 8), 4);
        assert_eq!(column_count(172.0, 954.0, 12.0, 5), 5);
        assert_eq!(column_count(172.0, 954.0, 12.0, 7), 5);
        assert_eq!(column_count(172.0, 100.0, 12.0, 8), 1);
    }

    #[test]
    fn window_widths_map_to_tiles() {
        assert_eq!(Width::of(400.0), Width::Narrow);
        assert_eq!(Width::of(752.0), Width::Medium);
        assert_eq!(Width::of(1516.0), Width::Wide);
    }
}
