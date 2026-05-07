// Generated automatically by iced_lucide at build time.
// Do not edit manually.
// 531fcd034de63999710de0d9ad750b7c8166c048197c0f3c85ee4614efd39366
use iced::widget::{text, Text};
use iced::Font;

pub const FONT: &[u8] = include_bytes!("../fonts/lucide.ttf");

/// All icons as `(name, codepoint_str)` pairs.
/// Use this to populate an icon-picker widget.
#[allow(dead_code)]
pub const ALL_ICONS: &[(&str, &str)] = &[
    ("arrow_left", "\u{E048}"),
    ("copy", "\u{E09E}"),
    ("eraser", "\u{E28F}"),
    ("info", "\u{E0F9}"),
    ("keyboard", "\u{E284}"),
    ("pencil", "\u{E1F9}"),
    ("plus", "\u{E13D}"),
    ("save", "\u{E14D}"),
    ("settings", "\u{E154}"),
    ("star", "\u{E176}"),
    ("trash", "\u{E18E}"),
    ("x", "\u{E1B2}"),
];

pub fn arrow_left<'a>() -> Text<'a> {
    icon("\u{E048}")
}

pub fn copy<'a>() -> Text<'a> {
    icon("\u{E09E}")
}

pub fn eraser<'a>() -> Text<'a> {
    icon("\u{E28F}")
}

pub fn info<'a>() -> Text<'a> {
    icon("\u{E0F9}")
}

pub fn keyboard<'a>() -> Text<'a> {
    icon("\u{E284}")
}

pub fn pencil<'a>() -> Text<'a> {
    icon("\u{E1F9}")
}

pub fn plus<'a>() -> Text<'a> {
    icon("\u{E13D}")
}

pub fn save<'a>() -> Text<'a> {
    icon("\u{E14D}")
}

pub fn settings<'a>() -> Text<'a> {
    icon("\u{E154}")
}

pub fn star<'a>() -> Text<'a> {
    icon("\u{E176}")
}

pub fn trash<'a>() -> Text<'a> {
    icon("\u{E18E}")
}

pub fn x<'a>() -> Text<'a> {
    icon("\u{E1B2}")
}

/// Render any Lucide icon by its codepoint string.
/// Use this together with [`ALL_ICONS`] to display icons dynamically:
/// ```ignore
/// for (name, cp) in ALL_ICONS {
///     button(render(cp)).on_press(Msg::Pick(name.to_string()))
/// }
/// ```
pub fn render(codepoint: &str) -> Text<'_> {
    text(codepoint).font(Font::with_name("lucide"))
}

fn icon(codepoint: &str) -> Text<'_> {
    render(codepoint)
}
