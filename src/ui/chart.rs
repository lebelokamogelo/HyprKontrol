//! Drawings: readings, the battery gauge and previews.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{container, row, space};
use iced::{Color, Fill, Length, Point, Rectangle, Renderer, Size, border, mouse};

use super::Element;
use crate::theme::Theme;
use crate::theme::color::fade;
use crate::theme::style::{self, Tone};

/// A thin horizontal bar.
pub fn bar<'a, M: 'a>(fraction: f32, tone: Tone) -> Element<'a, M> {
    filled(fraction, tone, 8.0)
}

/// A bar that underlines a row of a list.
pub fn line<'a, M: 'a>(fraction: f32, tone: Tone) -> Element<'a, M> {
    filled(fraction, tone, 4.0)
}

fn filled<'a, M: 'a>(fraction: f32, tone: Tone, height: f32) -> Element<'a, M> {
    // Portions are integers, hence the fixed-point resolution.
    let filled = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let fill = container(space())
        .height(height)
        .width(Length::FillPortion(filled.max(1)))
        .style(style::meter(Some(tone)));
    let rest = space().width(Length::FillPortion((1000 - filled).max(1)));
    let content: Element<'a, M> =
        if filled == 0 { space().into() } else { row![fill, rest].into() };
    container(content).height(height).width(Fill).clip(true).style(style::meter(None)).into()
}

/// Recent values as an area, newest on the right.
pub struct Spark {
    pub values: Vec<f32>,
    pub max: f32,
    /// Number of values that span the full width.
    pub span: usize,
}

impl Spark {
    pub fn view<'a, M: 'a>(self, height: f32) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(height).into()
    }
}

impl<M> canvas::Program<M, Theme> for Spark {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let c = theme.colors();
        let mut frame = Frame::new(renderer, bounds.size());
        // Room for the end marker, which would otherwise be cut in half.
        let (width, height) = (bounds.width - 6.0, bounds.height);

        for y in [0.5, height - 0.5] {
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(width, y)),
                Stroke::default().with_color(c.line).with_width(1.0),
            );
        }
        if self.values.len() < 2 || self.max <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let step = width / (self.span.max(2) - 1) as f32;
        let start = width - step * (self.values.len() - 1) as f32;
        let points: Vec<_> = self
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let y = height - 1.0 - (value / self.max).clamp(0.0, 1.0) * (height - 8.0);
                Point::new(start + step * index as f32, y)
            })
            .collect();
        let line = Path::new(|path| {
            path.move_to(points[0]);
            points[1..].iter().for_each(|point| path.line_to(*point));
        });
        let area = Path::new(|path| {
            path.move_to(Point::new(points[0].x, height));
            points.iter().for_each(|point| path.line_to(*point));
            path.line_to(Point::new(width, height));
            path.close();
        });
        frame.fill(&area, fade(c.live, 0.16));
        frame.stroke(
            &line,
            Stroke::default()
                .with_color(c.live)
                .with_width(2.0)
                .with_line_join(canvas::LineJoin::Round),
        );
        if let Some(end) = points.last() {
            frame.fill(&Path::circle(*end, 7.0), c.card);
            frame.fill(&Path::circle(*end, 4.5), c.live);
        }
        vec![frame.into_geometry()]
    }
}

/// Charge against the capacity the cell had when it was new. The hatched part is
/// what age has taken.
pub struct Gauge {
    pub now: f32,
    pub full: f32,
    pub design: f32,
}

impl Gauge {
    pub fn view<'a, M: 'a>(self) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(54).into()
    }
}

impl<M> canvas::Program<M, Theme> for Gauge {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let c = theme.colors();
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);
        let radius = theme.metrics().control_radius.min(height / 2.0);
        let total = self.design.max(self.full).max(0.1);
        let full = width * self.full / total;
        let now = width * self.now / total;
        let worn = width - full > 4.0;

        if worn {
            // Diagonals are cut to the region by hand. The two renderers disagree
            // on the origin of a clipped frame.
            let stroke = Stroke::default().with_color(c.track).with_width(1.6);
            let mut x = full - height;
            while x < width {
                let from = (full - x).max(0.0);
                let to = (width - x).min(height);
                if to > from {
                    frame.stroke(
                        &Path::line(
                            Point::new(x + from, height - from),
                            Point::new(x + to, height - to),
                        ),
                        stroke,
                    );
                }
                x += 9.0;
            }
            // Strokes cannot be clipped to a rounded shape, so the corners are
            // painted over with the colour behind the gauge.
            if radius > 0.0 {
                for y in [0.0, height] {
                    let inward = if y == 0.0 { radius } else { height - radius };
                    let corner = Path::new(|path| {
                        path.move_to(Point::new(width, y));
                        path.line_to(Point::new(width - radius, y));
                        path.quadratic_curve_to(Point::new(width, y), Point::new(width, inward));
                        path.close();
                    });
                    frame.fill(&corner, c.card);
                }
            }
        }

        let right = if worn { 0.0 } else { radius };
        let usable = border::Radius::default().left(radius).right(right);
        let usable_width = if worn { full - 2.0 } else { full };
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, Size::new(usable_width, height), usable),
            c.track,
        );
        if now > 1.0 {
            let end = if now >= usable_width - radius { right } else { 0.0 };
            let charge = border::Radius::default().left(radius.min(now)).right(end);
            frame.fill(
                &Path::rounded_rectangle(
                    Point::ORIGIN,
                    Size::new(now.min(usable_width), height),
                    charge,
                ),
                c.accent,
            );
        }
        vec![frame.into_geometry()]
    }
}

/// Progress through parts of different weight, such as the files of a download.
pub struct Parts {
    /// Weight of each part and how much of it is done.
    pub parts: Vec<(f32, f32)>,
}

impl Parts {
    pub fn view<'a, M: 'a>(self) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(54).into()
    }
}

impl<M> canvas::Program<M, Theme> for Parts {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        const GAP: f32 = 3.0;
        /// Below this, the gaps would say more than the parts.
        const NARROWEST: f32 = 12.0;

        let c = theme.colors();
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);
        let radius = theme.metrics().control_radius.min(height / 2.0);

        let total = self.parts.iter().map(|(weight, _)| weight).sum::<f32>().max(f32::EPSILON);
        let done = self.parts.iter().map(|(weight, done)| weight * done).sum::<f32>();
        let crowded = width < NARROWEST * self.parts.len() as f32;
        let merged = [(1.0, done / total)];
        let (parts, total) =
            if crowded { (merged.as_slice(), 1.0) } else { (self.parts.as_slice(), total) };

        let last = parts.len().saturating_sub(1);
        let free = width - GAP * last as f32;
        let mut x = 0.0;
        for (index, (weight, fraction)) in parts.iter().enumerate() {
            let part = free * weight / total;
            let left = if index == 0 { radius } else { 0.0 };
            let right = if index == last { radius } else { 0.0 };
            let mut fill = |width: f32, color: Color| {
                // A sliver cannot carry a full corner.
                let left = left.min(width);
                let right = if width >= part - 0.5 { right } else { 0.0 };
                let corners = border::Radius::default().left(left).right(right);
                let size = Size::new(width, height);
                frame.fill(&Path::rounded_rectangle(Point::new(x, 0.0), size, corners), color);
            };
            fill(part, c.track);
            let filled = part * fraction.clamp(0.0, 1.0);
            if filled > 0.5 {
                fill(filled, c.accent);
            }
            x += part + GAP;
        }
        vec![frame.into_geometry()]
    }
}

/// Load of every hardware thread, with a wider gap between kinds of cores.
pub struct Threads {
    pub loads: Vec<f32>,
    /// Number of threads in each group.
    pub groups: Vec<usize>,
}

impl Threads {
    pub fn view<'a, M: 'a>(self, height: f32) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(height).into()
    }
}

/// Space between two groups of threads, shared with the labels below them.
pub const GROUP_GAP: f32 = 24.0;

impl<M> canvas::Program<M, Theme> for Threads {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let c = theme.colors();
        let mut frame = Frame::new(renderer, bounds.size());
        let count = self.loads.len().max(1);
        let gap = if bounds.width < 420.0 { 4.0 } else { 8.0 };
        let breaks = self.groups.len().saturating_sub(1) as f32;
        let free = bounds.width - gap * (count - 1) as f32 - (GROUP_GAP - gap) * breaks;
        let bar = (free / count as f32).max(2.0);
        let radius = theme.metrics().control_radius.min(5.0);

        let mut x = 0.0;
        let mut boundary = self.groups.first().copied().unwrap_or(count);
        let mut groups = self.groups.iter().skip(1);
        for (index, load) in self.loads.iter().enumerate() {
            if index == boundary {
                x += GROUP_GAP - gap;
                boundary += groups.next().copied().unwrap_or(count);
            }
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x, 0.0),
                    Size::new(bar, bounds.height),
                    radius.into(),
                ),
                c.track,
            );
            let filled = load.clamp(0.0, 1.0) * bounds.height;
            if filled > 0.5 {
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(x, bounds.height - filled),
                        Size::new(bar, filled),
                        radius.min(filled / 2.0).into(),
                    ),
                    c.live,
                );
            }
            x += bar + gap;
        }
        vec![frame.into_geometry()]
    }
}

/// Three tiled windows with the given decoration, drawn at actual size.
pub struct Tiling {
    pub gaps_in: f32,
    pub gaps_out: f32,
    pub border: f32,
    pub rounding: f32,
    pub active: (Color, Color),
    pub inactive: Color,
    pub inactive_opacity: f32,
}

impl Tiling {
    pub fn view<'a, M: 'a>(self, height: f32) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(height).into()
    }
}

/// Wallpaper colours of the preview. They stand for the user's desktop, so they
/// intentionally do not follow the theme of this program.
const SCENE: [Color; 3] = [
    Color::from_rgb(0.04, 0.20, 0.25),
    Color::from_rgb(0.0, 0.41, 0.37),
    Color::from_rgb(0.0, 0.54, 0.45),
];

impl<M> canvas::Program<M, Theme> for Tiling {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let Size { width, height } = bounds.size();
        let corner = theme.metrics().card_radius;
        let scene = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), corner.into());
        frame.fill(&scene, SCENE[0]);
        for (color, level, depth) in [(SCENE[1], 0.62, 46.0), (SCENE[2], 0.8, 34.0)] {
            let y = height * level;
            let hill = Path::new(|path| {
                path.move_to(Point::new(0.0, y));
                path.bezier_curve_to(
                    Point::new(width * 0.3, y - depth * 2.0),
                    Point::new(width * 0.55, y + depth * 2.0),
                    Point::new(width, y - depth),
                );
                path.line_to(Point::new(width, height - corner));
                path.quadratic_curve_to(
                    Point::new(width, height),
                    Point::new(width - corner, height),
                );
                path.line_to(Point::new(corner, height));
                path.quadratic_curve_to(Point::new(0.0, height), Point::new(0.0, height - corner));
                path.close();
            });
            frame.fill(&hill, color);
        }

        let (out, gap) = (self.gaps_out, self.gaps_in);
        let middle = width / 2.0;
        let half = height / 2.0;
        let left =
            Rectangle { x: out, y: out, width: middle - out - gap, height: height - 2.0 * out };
        let top = Rectangle {
            x: middle + gap,
            y: out,
            width: width - middle - gap - out,
            height: half - out - gap,
        };
        let bottom = Rectangle { y: half + gap, ..top };
        self.window(&mut frame, left, true);
        self.window(&mut frame, top, false);
        self.window(&mut frame, bottom, false);
        vec![frame.into_geometry()]
    }
}

impl Tiling {
    fn window(&self, frame: &mut Frame, area: Rectangle, active: bool) {
        if area.width <= 4.0 || area.height <= 4.0 {
            return;
        }
        let rounding = self.rounding.min(area.width / 2.0).min(area.height / 2.0);
        let size = area.size();
        let outer = Path::rounded_rectangle(area.position(), size, rounding.into());
        if self.border > 0.0 {
            if active {
                let gradient = canvas::gradient::Linear::new(
                    Point::new(area.x, area.y + area.height),
                    Point::new(area.x + area.width, area.y),
                )
                .add_stop(0.0, self.active.0)
                .add_stop(1.0, self.active.1);
                frame.fill(&outer, gradient);
            } else {
                frame.fill(&outer, self.inactive);
            }
        }
        let inner = Path::rounded_rectangle(
            Point::new(area.x + self.border, area.y + self.border),
            Size::new(area.width - 2.0 * self.border, area.height - 2.0 * self.border),
            (rounding - self.border).max(0.0).into(),
        );
        let opacity = if active { 1.0 } else { self.inactive_opacity };
        frame.fill(&inner, Color { a: opacity, ..Color::from_rgb(0.067, 0.094, 0.149) });

        // A few lines of text are enough to read the shape as a window.
        for (index, share) in [0.5, 0.36, 0.62, 0.28].into_iter().enumerate() {
            let y = area.y + self.border + 20.0 + index as f32 * 18.0;
            if y + 14.0 > area.y + area.height {
                break;
            }
            let tone = if index == 0 { self.active.0 } else { Color::from_rgb(0.35, 0.42, 0.56) };
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(area.x + self.border + 18.0, y),
                    Size::new((area.width - 2.0 * self.border - 36.0).max(0.0) * share, 6.0),
                    3.0.into(),
                ),
                fade(tone, opacity),
            );
        }
    }
}

/// The arrangement of displays. Clicking one selects it.
pub struct Screens<M> {
    /// Position, size and name of each display on the desktop.
    pub screens: Vec<(Rectangle, String)>,
    pub selected: usize,
    pub on_select: fn(usize) -> M,
}

impl<M: 'static> Screens<M> {
    pub fn view<'a>(self, height: f32) -> Element<'a, M> {
        Canvas::new(self).width(Fill).height(height).into()
    }

    /// Scales the desktop into `bounds`, centred.
    fn placed(&self, bounds: Size) -> Vec<Rectangle> {
        let edge = |pick: fn(&Rectangle) -> f32, best: fn(f32, f32) -> f32, start: f32| {
            self.screens.iter().map(|(area, _)| pick(area)).fold(start, best)
        };
        let left = edge(|r| r.x, f32::min, f32::MAX);
        let top = edge(|r| r.y, f32::min, f32::MAX);
        let right = edge(|r| r.x + r.width, f32::max, f32::MIN);
        let bottom = edge(|r| r.y + r.height, f32::max, f32::MIN);
        let (width, height) = ((right - left).max(1.0), (bottom - top).max(1.0));
        let scale = ((bounds.width - 48.0) / width).min((bounds.height - 48.0) / height);
        let offset = ((bounds.width - width * scale) / 2.0, (bounds.height - height * scale) / 2.0);
        self.screens
            .iter()
            .map(|(area, _)| Rectangle {
                x: offset.0 + (area.x - left) * scale + 2.0,
                y: offset.1 + (area.y - top) * scale + 2.0,
                width: area.width * scale - 4.0,
                height: area.height * scale - 4.0,
            })
            .collect()
    }
}

impl<M> canvas::Program<M, Theme> for Screens<M>
where
    M: 'static,
{
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let position = cursor.position_in(bounds)?;
        let hit = self.placed(bounds.size()).iter().position(|area| area.contains(position))?;
        Some(canvas::Action::publish((self.on_select)(hit)).and_capture())
    }

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let c = theme.colors();
        let mut frame = Frame::new(renderer, bounds.size());
        let radius = theme.metrics().control_radius.min(10.0);
        for (index, area) in self.placed(bounds.size()).into_iter().enumerate() {
            let selected = index == self.selected;
            let shape = Path::rounded_rectangle(area.position(), area.size(), radius.into());
            frame.fill(&shape, if selected { c.raised } else { c.track });
            if selected {
                frame.stroke(&shape, Stroke::default().with_color(c.accent).with_width(2.0));
            }
            frame.fill_text(canvas::Text {
                content: self.screens[index].1.clone(),
                position: area.center(),
                color: c.text,
                size: theme.metrics().text_size.into(),
                font: theme.font(iced::font::Weight::Medium),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        (): &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(position) if self.placed(bounds.size()).iter().any(|a| a.contains(position)) => {
                mouse::Interaction::Pointer
            }
            _ => mouse::Interaction::default(),
        }
    }
}
