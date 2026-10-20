// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use super::style;
use crate::{model::Scene, res};
use day::prelude::*;

pub fn browser() -> impl Piece {
    let s = Scene::ambient();
    column((
        column((
            label(res::str::browse())
                .font(Font::Title2)
                .bold()
                .id(day::id!(browser_title)),
            label(res::str::browse_caption())
                .font(Font::Footnote)
                .secondary(),
            text_field(s.kind)
                .placeholder(res::str::type_hint())
                .a11y(|a| a.label(res::str::service_type()))
                .id(day::id!(browse_type)),
            row((
                button(res::str::start())
                    .action(move || s.browse())
                    .id(day::id!(browse_start))
                    .grow_w(),
                button(res::str::stop())
                    .action(move || s.stop_browse())
                    .enabled(move || s.browsing.get())
                    .id(day::id!(browse_stop)),
            ))
            .spacing(8.0)
            .grow_w(),
            label(s.status)
                .font(Font::Caption)
                .secondary()
                .id(day::id!(browse_status)),
            text_field(s.filter)
                .placeholder(res::str::search_hint())
                .a11y(|a| a.label(res::str::search()))
                .id(day::id!(service_search)),
            row((
                label(move || res::str::service_count(s.rows().len().to_string()).format())
                    .font(Font::Footnote)
                    .bold()
                    .id(day::id!(service_count))
                    .grow_w(),
                when(
                    move || !s.rows().is_empty(),
                    || {
                        label(res::str::services_found())
                            .font(Font::Caption)
                            .secondary()
                            .id(day::id!(services_found))
                    },
                ),
            ))
            .spacing(8.0)
            .grow_w(),
            when(
                move || s.rows().is_empty(),
                || {
                    section((label(res::str::empty())
                        .font(Font::Callout)
                        .secondary()
                        .id(day::id!(services_empty)),))
                },
            ),
        ))
        .spacing(12.0)
        .align(HAlign::Leading)
        .padding(16.0),
        list(
            items(
                move || s.rows(),
                |id: &day_part_zeroconf::ServiceId| id.clone(),
            ),
            move |slot| {
                row((
                    style::emblem(move || style::accent(&slot.get().service_type)),
                    column((
                        label(move || slot.get().name)
                            .font(Font::Callout)
                            .single_line()
                            .bold(),
                        label(move || slot.get().service_type)
                            .font(Font::Caption)
                            .single_line()
                            .monospace()
                            .secondary(),
                        label(move || {
                            res::str::interface_label(slot.get().interface.to_string()).format()
                        })
                        .font(Font::Caption2)
                        .secondary(),
                    ))
                    .spacing(4.0)
                    .align(HAlign::Leading)
                    .grow_w(),
                ))
                .spacing(12.0)
                .align(VAlign::Top)
                .padding(Insets::symmetric(12.0, 16.0))
                .grow_w()
            },
        )
        .row_height(RowHeight::Uniform(88.0))
        .on_select(move |id| s.select(id))
        .selected_rows(move || {
            s.selected
                .get()
                .and_then(|id| s.rows().iter().position(|row| *row == id))
                .into_iter()
                .collect()
        })
        .a11y(|a| a.label(res::str::services()))
        .id(day::id!(service_list))
        .grow(),
    ))
    .align(HAlign::Leading)
    .grow()
}
