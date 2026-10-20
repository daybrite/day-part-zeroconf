// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
mod browser;
mod inspector;
mod style;
use crate::{model::Scene, res};
pub use browser::browser;
use day::prelude::*;
pub use inspector::inspector;
pub fn publisher() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::publish())
                .font(Font::Title)
                .id(day::id!(publisher_title)),
            label(res::str::publish_caption()).font(Font::Footnote),
            section((
                labeled(
                    res::str::instance_name(),
                    text_field(s.name).id(day::id!(publish_name)),
                ),
                labeled(
                    res::str::service_type(),
                    text_field(s.publish_kind).id(day::id!(publish_type)),
                ),
                labeled(
                    res::str::port(),
                    text_field(s.port)
                        .placeholder(res::str::port_hint())
                        .id(day::id!(publish_port)),
                ),
                labeled(
                    res::str::txt_records(),
                    text_area(s.txt)
                        .min_lines(3)
                        .max_lines(6)
                        .spellcheck(false)
                        .id(day::id!(publish_txt)),
                ),
                label(res::str::txt_hint()).font(Font::Footnote),
                row((
                    button(res::str::advertise())
                        .enabled(move || !s.publishing.get())
                        .action(move || s.publish())
                        .id(day::id!(publish_start)),
                    button(res::str::withdraw())
                        .enabled(move || s.publishing.get())
                        .action(move || s.stop_publish())
                        .id(day::id!(publish_stop)),
                ))
                .spacing(8.0),
                label(s.publish_status)
                    .selectable()
                    .id(day::id!(publish_status)),
                when(
                    move || s.published.get(),
                    || label(res::str::advertisement_active()).id(day::id!(advertisement_active)),
                ),
            ))
            .title(res::str::advertisement()),
        ))
        .spacing(16.0)
        .align(HAlign::Leading)
        .padding(20.0),
    )
}
