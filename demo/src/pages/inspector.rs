// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use super::style;
use crate::{model::Scene, network, res};
use day::prelude::*;

pub fn inspector() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::inspector())
                .font(Font::Footnote)
                .secondary()
                .id("inspector-title"),
            when(
                move || s.selected.get().is_none(),
                || {
                    section((
                        style::emblem(|| style::accent("")),
                        label(res::str::inspector_empty_title())
                            .font(Font::Title2)
                            .bold(),
                        label(res::str::select_service()).secondary(),
                    ))
                },
            ),
            when(
                move || s.selected.get().is_some(),
                move || {
                    column((
                        row((
                            style::emblem(move || {
                                style::accent(
                                    &s.selected
                                        .get()
                                        .map(|id| id.service_type)
                                        .unwrap_or_default(),
                                )
                            }),
                            label(move || {
                                s.selected
                                    .get()
                                    .map(|id| id.service_type)
                                    .unwrap_or_default()
                            })
                            .font(Font::Footnote)
                            .monospace()
                            .secondary()
                            .selectable()
                            .grow_w(),
                        ))
                        .spacing(12.0)
                        .align(VAlign::Center)
                        .grow_w(),
                        label(move || s.selected.get().map(|id| id.name).unwrap_or_default())
                            .font(Font::Title2)
                            .bold()
                            .id("service-name"),
                        label(move || {
                            s.selected
                                .get()
                                .map(|id| {
                                    res::str::service_location(id.domain, id.interface.to_string())
                                        .format()
                                })
                                .unwrap_or_default()
                        })
                        .font(Font::Footnote)
                        .secondary()
                        .selectable(),
                        when(
                            move || s.service().is_none(),
                            || label(res::str::resolving()).id("service-resolving"),
                        ),
                        when(
                            move || s.service().is_some(),
                            move || {
                                column((
                                    section((
                                        label(move || {
                                            s.service()
                                                .map(|v| {
                                                    res::str::endpoint(
                                                        v.hostname,
                                                        v.port.to_string(),
                                                    )
                                                    .format()
                                                })
                                                .unwrap_or_default()
                                        })
                                        .font(Font::Callout)
                                        .monospace()
                                        .selectable()
                                        .id("service-endpoint"),
                                        when(
                                            move || {
                                                s.service().is_some_and(|v| {
                                                    v.addresses.iter().any(|a| a.is_ipv4())
                                                })
                                            },
                                            || {
                                                label(res::str::connection_ready())
                                                    .font(Font::Caption)
                                                    .secondary()
                                                    .id("connection-ready")
                                            },
                                        ),
                                        row((
                                            button(res::str::tcp_connect())
                                                .enabled(move || {
                                                    !s.probing.get()
                                                        && s.service().is_some_and(|v| {
                                                            !v.addresses.is_empty()
                                                                && v.id
                                                                    .service_type
                                                                    .ends_with("._tcp")
                                                        })
                                                })
                                                .action(move || s.connect(false))
                                                .id("tcp-connect"),
                                            button(res::str::http_get())
                                                .enabled(move || {
                                                    !s.probing.get()
                                                        && s.service().is_some_and(|v| {
                                                            !v.addresses.is_empty()
                                                                && network::can_http(&v)
                                                        })
                                                })
                                                .action(move || s.connect(true))
                                                .id("http-get"),
                                        ))
                                        .spacing(8.0)
                                        .fit(RowFit::Wrap { run_spacing: 8.0 }),
                                        label(res::str::probe_caption())
                                            .font(Font::Caption)
                                            .secondary(),
                                        when(
                                            move || s.probe_ok.get(),
                                            || {
                                                label(res::str::connection_ok())
                                                    .font(Font::Footnote)
                                                    .bold()
                                                    .color(Color::hex(0xffffff))
                                                    .padding(8.0)
                                                    .background(Color::hex(0x167558))
                                                    .corner_radius(8.0)
                                                    .id("connection-ok")
                                            },
                                        ),
                                        when(
                                            move || !s.probe.get().is_empty(),
                                            move || {
                                                style::code_panel(
                                                    move || s.probe.get(),
                                                    "probe-result",
                                                    Some(180.0),
                                                )
                                            },
                                        ),
                                    ))
                                    .title(res::str::connections()),
                                    column((
                                        label(res::str::addresses()).font(Font::Headline),
                                        style::code_panel(
                                            move || {
                                                s.service()
                                                    .map(|v| {
                                                        v.addresses
                                                            .iter()
                                                            .map(ToString::to_string)
                                                            .collect::<Vec<_>>()
                                                            .join("\n")
                                                    })
                                                    .unwrap_or_default()
                                            },
                                            "service-addresses",
                                            None,
                                        ),
                                    ))
                                    .spacing(8.0)
                                    .align(HAlign::Leading),
                                    column((
                                        label(res::str::txt_records()).font(Font::Headline),
                                        label(res::str::data_scroll_hint())
                                            .font(Font::Caption)
                                            .secondary(),
                                        style::code_panel(
                                            move || {
                                                s.service()
                                                    .map(|v| {
                                                        if v.txt.0.is_empty() {
                                                            return res::str::no_txt().format();
                                                        }
                                                        v.txt
                                                            .0
                                                            .iter()
                                                            .map(|(k, v)| match v {
                                                                None => {
                                                                    res::str::txt_flag(k.clone())
                                                                        .format()
                                                                }
                                                                Some(v) => {
                                                                    match std::str::from_utf8(v) {
                                                                        Ok(text)
                                                                            if !text
                                                                                .chars()
                                                                                .any(
                                                                                char::is_control,
                                                                            ) =>
                                                                        {
                                                                            format!("{k}={text}")
                                                                        }
                                                                        _ => format!(
                                                                            "{k}=0x{}",
                                                                            v.iter()
                                                                                .map(|b| format!(
                                                                                    "{b:02x}"
                                                                                ))
                                                                                .collect::<String>(
                                                                                )
                                                                        ),
                                                                    }
                                                                }
                                                            })
                                                            .collect::<Vec<_>>()
                                                            .join("\n")
                                                    })
                                                    .unwrap_or_default()
                                            },
                                            "service-txt",
                                            None,
                                        ),
                                    ))
                                    .spacing(8.0)
                                    .align(HAlign::Leading),
                                ))
                                .spacing(24.0)
                                .align(HAlign::Leading)
                            },
                        ),
                    ))
                    .spacing(16.0)
                    .align(HAlign::Leading)
                },
            ),
            section((
                label(res::str::platform_notes())
                    .font(Font::Caption)
                    .secondary(),
                when(
                    move || !s.log.get().is_empty(),
                    move || {
                        style::code_panel(
                            move || {
                                s.log
                                    .get()
                                    .into_iter()
                                    .rev()
                                    .take(15)
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            },
                            "event-log",
                            Some(140.0),
                        )
                    },
                ),
            ))
            .title(res::str::activity()),
        ))
        .spacing(24.0)
        .align(HAlign::Leading)
        .padding(20.0),
    )
    .id("inspector-scroll")
}
