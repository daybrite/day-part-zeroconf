// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::res;
use day::prelude::*;

pub fn accent(kind: &str) -> Color {
    let hex = if kind.contains("http") || kind.contains("opds") {
        0x2764c5
    } else if kind.contains("airplay") || kind.contains("spotify") || kind.contains("raop") {
        0x8250b5
    } else if kind.contains("ipp") || kind.contains("printer") {
        0xa75a20
    } else {
        0x167d89
    };
    Color::hex(hex)
}

pub fn emblem(color: impl Fn() -> Color + 'static) -> impl Piece {
    vector(res::vectors::network)
        .tint(Color::hex(0xffffff))
        .frame(20.0, 20.0)
        .padding(10.0)
        .background(color)
        .corner_radius(12.0)
}

/// Native scroll views preserve long protocol lines, including IPv6 scope/port suffixes.
/// The dark inset surface and its foreground are a pair, independent of system appearance.
pub fn code_panel(
    text: impl Fn() -> String + 'static,
    id: &'static str,
    height: Option<f64>,
) -> impl Piece {
    let text = std::rc::Rc::new(text);
    let measure_text = text.clone();
    // Day's scroll viewport needs an explicit vertical extent. Rebuild only when the
    // visible line count changes; same-height value updates preserve the scroll position.
    each(
        items(
            move || {
                vec![if height.is_some() {
                    0
                } else {
                    measure_text().lines().count().clamp(2, 10)
                }]
            },
            |lines: &usize| *lines,
        ),
        move |slot| {
            let text = text.clone();
            scroll(
                scroll(
                    label(move || text())
                        .font(Font::System(13.0))
                        .monospace()
                        .color(Color::hex(0xd8ecf5))
                        .selectable()
                        .id(id)
                        .padding(14.0),
                )
                .horizontal()
                .id(format!("{id}-horizontal")),
            )
            .id(format!("{id}-vertical"))
            .height(height.unwrap_or(slot.get() as f64 * 20.0 + 28.0))
            .background(Color::hex(0x172536))
            .corner_radius(12.0)
            .grow_w()
        },
    )
}
