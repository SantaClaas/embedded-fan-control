//! Just enough SVG to draw a wiring diagram: boxes with pins on their edges,
//! orthogonal wires between them, and the handful of parts that are drawn as
//! themselves rather than as a box.
//!
//! Coordinates are the drawing's own, in the order they are read: x to the
//! right, y down, and every sheet declares its own row and column constants.

use std::fmt::Write as _;

pub const BG: &str = "#ffffff";
pub const BOX_FILL: &str = "#f6f6f3";
pub const STROKE: &str = "#3a3a3a";
pub const TEXT: &str = "#1a1a1a";
pub const MUTED: &str = "#5c5c5c";
pub const FONT: &str = "ui-sans-serif, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif";
pub const MONO: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/// The resistor body itself, so the bands on it are legible in the way the
/// part in your hand is.
pub const BODY: &str = "#efe3c8";

/// The band colours, black through white, in the order the digits run. Gold is
/// the tolerance band and is not a digit.
const DIGITS: [&str; 10] = [
    "#1c1c1c", // black
    "#7a4a21", // brown
    "#c02a1f", // red
    "#e07b20", // orange
    "#e6c229", // yellow
    "#2f8f46", // green
    "#2f5fbf", // blue
    "#7d4fbf", // violet
    "#9a9a9a", // grey
    "#fbfbfb", // white
];
const GOLD: &str = "#c9a227";

/// The four bands of a value in ohms, read from the end the bands are crowded
/// towards: two digits, the power of ten to multiply them by, and gold for the
/// 5 % tolerance the parts here are.
///
/// Only values with two significant figures are drawn, which is every value on
/// these sheets and most of the E24 series besides.
fn colour_code(ohms: u32) -> [&'static str; 4] {
    let (mut digits, mut exponent) = (ohms, 0usize);
    while digits >= 100 {
        digits /= 10;
        exponent += 1;
    }
    [
        DIGITS[(digits / 10) as usize],
        DIGITS[(digits % 10) as usize],
        DIGITS[exponent],
        GOLD,
    ]
}

/// How a run of text is set: the face, the size in points before the sheet's
/// own scale, and the colour.
#[derive(Clone, Copy)]
struct Style {
    font: &'static str,
    size: f64,
    fill: &'static str,
}

impl Style {
    /// Pin names and component values, in the face a register dump is read in.
    fn mono(size: f64) -> Self {
        Self {
            font: MONO,
            size,
            fill: TEXT,
        }
    }

    /// An annotation rather than part of the circuit.
    fn note(size: f64) -> Self {
        Self {
            font: FONT,
            size,
            fill: MUTED,
        }
    }
}

/// Which side of a box edge a pin's label sits on.
#[derive(Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

impl Anchor {
    fn as_str(self) -> &'static str {
        match self {
            Anchor::Start => "start",
            Anchor::Middle => "middle",
            Anchor::End => "end",
        }
    }
}

/// Where a bridging resistor's value is written.
#[derive(Clone, Copy)]
pub enum Label {
    Right,
    Below,
}

/// A number as the drawing writes it: whole values without a trailing zero, so
/// the output stays readable when the file is opened rather than rendered.
fn n(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        let mut text = format!("{value}");
        if text.ends_with(".0") {
            text.truncate(text.len() - 2);
        }
        text
    }
}

pub struct Sheet {
    width: f64,
    height: f64,
    title: String,
    description: String,
    /// Font scale, for a sheet that gets shrunk to fit the page it is read on.
    font_scale: f64,
    body: String,
}

impl Sheet {
    pub fn new(width: f64, height: f64, title: &str, description: &str, font_scale: f64) -> Self {
        Self {
            width,
            height,
            title: title.to_string(),
            description: description.to_string(),
            font_scale,
            body: String::new(),
        }
    }

    pub fn add(&mut self, element: String) {
        writeln!(self.body, "  {element}").expect("writing to a String cannot fail");
    }

    fn size(&self, points: f64) -> String {
        format!("{:.1}", points * self.font_scale)
    }

    fn text(&mut self, x: f64, y: f64, anchor: Anchor, style: Style, body: &str) {
        let Style { font, size, fill } = style;
        let (x, y, size) = (n(x), n(y), self.size(size));
        self.add(format!(
            r#"<text x="{x}" y="{y}" text-anchor="{}" font-family="{font}" font-size="{size}" fill="{fill}">{body}</text>"#,
            anchor.as_str()
        ));
    }

    /// A device: its outline, its name, and optionally a second line under it.
    pub fn device(&mut self, x: f64, y: f64, w: f64, h: f64, name: &str, note: Option<&str>) {
        self.add(format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="6" fill="{BOX_FILL}" stroke="{STROKE}" stroke-width="1.5"/>"#,
            n(x), n(y), n(w), n(h)
        ));
        let size = self.size(15.0);
        let (cx, ty) = (n(x + w / 2.0), n(y + 24.0));
        self.add(format!(
            r#"<text x="{cx}" y="{ty}" text-anchor="middle" font-family="{FONT}" font-size="{size}" font-weight="600" fill="{TEXT}">{name}</text>"#
        ));
        if let Some(note) = note {
            self.text(
                x + w / 2.0,
                y + 43.0,
                Anchor::Middle,
                Style::note(12.5),
                note,
            );
        }
    }

    /// A pin label, just inside the box edge it belongs to.
    pub fn pin(&mut self, x: f64, y: f64, label: &str, side: Side) {
        let (dx, anchor) = match side {
            Side::Left => (8.0, Anchor::Start),
            Side::Right => (-8.0, Anchor::End),
        };
        self.text(x + dx, y + 4.5, anchor, Style::mono(12.5), label);
    }

    /// A wire, as the corners it turns. An arrow marks the end that drives it.
    pub fn wire(&mut self, points: &[(f64, f64)], arrow: bool) {
        let mut d = String::new();
        for (index, (x, y)) in points.iter().enumerate() {
            let command = if index == 0 { "M" } else { "L" };
            let _ = write!(
                d,
                "{}{command}{} {}",
                if index == 0 { "" } else { " " },
                n(*x),
                n(*y)
            );
        }
        let marker = if arrow {
            r#" marker-end="url(#arrow)""#
        } else {
            ""
        };
        self.add(format!(
            r#"<path d="{d}" fill="none" stroke="{STROKE}" stroke-width="1.6" stroke-linejoin="round"{marker}/>"#
        ));
    }

    /// A junction. Wires that cross without one of these are not connected.
    pub fn dot(&mut self, x: f64, y: f64) {
        self.add(format!(
            r#"<circle cx="{}" cy="{}" r="3.4" fill="{STROKE}"/>"#,
            n(x),
            n(y)
        ));
    }

    /// A vertical wire that hops over the horizontals it crosses. The hop is
    /// what says a crossing is not a junction.
    pub fn rail(&mut self, x: f64, from: f64, to: f64, crossings: &[f64]) {
        let mut crossings: Vec<f64> = crossings.to_vec();
        crossings.sort_by(|a, b| a.partial_cmp(b).expect("no coordinate is NaN"));
        let mut d = format!("M{} {}", n(x), n(from));
        for crossing in crossings {
            let _ = write!(
                d,
                " L{} {} A 6 6 0 0 0 {} {}",
                n(x),
                n(crossing - 6.0),
                n(x),
                n(crossing + 6.0)
            );
        }
        let _ = write!(d, " L{} {}", n(x), n(to));
        self.add(format!(
            r#"<path d="{d}" fill="none" stroke="{STROKE}" stroke-width="1.6"/>"#
        ));
    }

    /// A resistor bridging two horizontal wires at one column, as the bus
    /// terminators are.
    pub fn resistor(&mut self, x: f64, y1: f64, y2: f64, ohms: u32, place: Label) {
        let mid = (y1 + y2) / 2.0;
        self.wire(&[(x, y1), (x, mid - 15.0)], false);
        self.wire(&[(x, mid + 15.0), (x, y2)], false);
        self.add(format!(
            r#"<rect x="{}" y="{}" width="16" height="30" rx="2" fill="{BODY}" stroke="{STROKE}" stroke-width="1.5"/>"#,
            n(x - 8.0),
            n(mid - 15.0)
        ));
        // Three bands crowded towards the top, the tolerance band alone at the
        // bottom, which is the end you read from.
        for (band, offset) in colour_code(ohms).iter().zip([4.0, 9.0, 14.0, 23.0]) {
            self.add(format!(
                r#"<rect x="{}" y="{}" width="16" height="3" fill="{band}"/>"#,
                n(x - 8.0),
                n(mid - 15.0 + offset)
            ));
        }
        let label = format!("{ohms} Ω");
        match place {
            Label::Right => self.text(
                x + 15.0,
                mid + 4.5,
                Anchor::Start,
                Style::mono(12.5),
                &label,
            ),
            Label::Below => self.text(x, y2 + 26.0, Anchor::Middle, Style::mono(12.5), &label),
        }
        self.dot(x, y1);
        self.dot(x, y2);
    }

    /// A resistor in line with a horizontal wire, centred on x.
    pub fn resistor_inline(&mut self, x: f64, y: f64, ohms: u32) {
        self.add(format!(
            r#"<rect x="{}" y="{}" width="38" height="16" rx="2" fill="{BODY}" stroke="{STROKE}" stroke-width="1.5"/>"#,
            n(x - 19.0),
            n(y - 8.0)
        ));
        for (band, offset) in colour_code(ohms).iter().zip([5.0, 10.0, 15.0, 29.0]) {
            self.add(format!(
                r#"<rect x="{}" y="{}" width="3" height="16" fill="{band}"/>"#,
                n(x - 19.0 + offset),
                n(y - 8.0)
            ));
        }
        self.text(
            x,
            y - 15.0,
            Anchor::Middle,
            Style::mono(12.5),
            &format!("{ohms} Ω"),
        );
    }

    /// An LED in line with a horizontal wire, anode on the left.
    pub fn led(&mut self, x: f64, y: f64, label: &str) {
        self.add(format!(
            r#"<path d="M{} {} L{} {} L{} {} z" fill="{BOX_FILL}" stroke="{STROKE}" stroke-width="1.5" stroke-linejoin="round"/>"#,
            n(x - 9.0), n(y - 10.0), n(x + 8.0), n(y), n(x - 9.0), n(y + 10.0)
        ));
        self.add(format!(
            r#"<path d="M{} {} L{} {}" stroke="{STROKE}" stroke-width="1.8"/>"#,
            n(x + 8.0),
            n(y - 11.0),
            n(x + 8.0),
            n(y + 11.0)
        ));
        self.add(format!(
            r#"<path d="M{} {} l7 -7 M{} {} l7 -7" stroke="{STROKE}" stroke-width="1.2" marker-end="url(#arrow)"/>"#,
            n(x + 2.0), n(y - 14.0), n(x + 8.0), n(y - 18.0)
        ));
        self.text(x, y + 30.0, Anchor::Middle, Style::note(12.5), label);
    }

    /// A note in the margin, in the muted colour so it reads as annotation
    /// rather than as part of the circuit.
    pub fn note(&mut self, x: f64, y: f64, text: &str, anchor: Anchor) {
        self.text(x, y, anchor, Style::note(12.5), text);
    }

    /// A bracket tying two rows together, with a note beside it.
    pub fn brace(&mut self, x: f64, y1: f64, y2: f64, text: &str) {
        self.add(format!(
            r#"<path d="M{} {} L{} {} L{} {} L{} {}" fill="none" stroke="{MUTED}" stroke-width="1.2"/>"#,
            n(x), n(y1), n(x + 8.0), n(y1), n(x + 8.0), n(y2), n(x), n(y2)
        ));
        self.note(x + 14.0, (y1 + y2) / 2.0 + 4.5, text, Anchor::Start);
    }

    pub fn render(&self) -> String {
        let (w, h) = (n(self.width), n(self.height));
        format!(
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {0} {1}" width="{0}" height="{1}" role="img" aria-label="{2}">"#,
                "\n  <title>{2}</title>\n  <desc>{3}</desc>\n",
                "  <defs>\n",
                r#"    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">"#,
                "\n",
                r#"      <path d="M0 0 L10 5 L0 10 z" fill="{4}"/>"#,
                "\n    </marker>\n  </defs>\n",
                r#"  <rect width="{0}" height="{1}" fill="{5}"/>"#,
                "\n{6}</svg>\n"
            ),
            w, h, self.title, self.description, STROKE, BG, self.body
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DIGITS, GOLD, colour_code};

    #[test]
    fn reads_the_values_on_these_sheets() {
        let (brown, red, orange) = (DIGITS[1], DIGITS[2], DIGITS[3]);
        // 120 Ω: brown, red, brown, gold
        assert_eq!(colour_code(120), [brown, red, brown, GOLD]);
        // 330 Ω: orange, orange, brown, gold
        assert_eq!(colour_code(330), [orange, orange, brown, GOLD]);
    }

    #[test]
    fn counts_the_multiplier_rather_than_the_zeroes() {
        let (black, brown, red, yellow) = (DIGITS[0], DIGITS[1], DIGITS[2], DIGITS[4]);
        // 10 Ω is brown, black, black: the multiplier is one, not ten
        assert_eq!(colour_code(10), [brown, black, black, GOLD]);
        assert_eq!(colour_code(47), [yellow, DIGITS[7], black, GOLD]);
        assert_eq!(colour_code(4_700), [yellow, DIGITS[7], red, GOLD]);
        assert_eq!(colour_code(1_000_000), [brown, black, DIGITS[5], GOLD]);
    }
}
