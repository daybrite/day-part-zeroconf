// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
pub mod settings;
use crate::{model::Scene, network, res};
use day::prelude::*;
pub fn browser() -> impl Piece {
    let s = Scene::ambient();
    column((
        column((
            label(res::str::browse())
                .font(Font::Title)
                .id("browser-title"),
            text_field(s.kind)
                .placeholder(res::str::type_hint())
                .a11y(|a| a.label(res::str::service_type()))
                .id("browse-type"),
            row((
                button(res::str::start())
                    .action(move || s.browse())
                    .id("browse-start"),
                button(res::str::stop())
                    .action(move || s.stop_browse())
                    .enabled(move || s.browsing.get())
                    .id("browse-stop"),
            ))
            .spacing(8.0),
            label(s.status).font(Font::Footnote).id("browse-status"),
            text_field(s.filter)
                .placeholder(res::str::search_hint())
                .a11y(|a| a.label(res::str::search()))
                .id("service-search"),
            label(move || res::str::service_count(s.rows().len().to_string()).format())
                .secondary()
                .id("service-count"),
            when(
                move || !s.rows().is_empty(),
                || {
                    label(res::str::services_found())
                        .font(Font::Footnote)
                        .id("services-found")
                },
            ),
            when(
                move || s.rows().is_empty(),
                || {
                    label(res::str::empty())
                        .font(Font::Footnote)
                        .secondary()
                        .id("services-empty")
                },
            ),
        ))
        .spacing(8.0)
        .padding(12.0),
        list(
            items(
                move || s.rows(),
                |id: &day_part_zeroconf::ServiceId| id.clone(),
            ),
            move |slot| {
                column((
                    label(move || slot.get().name),
                    label(move || {
                        let id = slot.get();
                        format!("{} · {}", id.service_type, id.interface)
                    })
                    .font(Font::Footnote)
                    .secondary(),
                ))
                .spacing(3.0)
                .padding(10.0)
            },
        )
        .on_select(move |id| s.select(id))
        .selected_rows(move || {
            s.selected
                .get()
                .and_then(|id| s.rows().iter().position(|row| *row == id))
                .into_iter()
                .collect()
        })
        .a11y(|a| a.label(res::str::services()))
        .id("service-list")
        .grow(),
    ))
    .grow()
}
pub fn inspector() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::inspector())
                .font(Font::Title)
                .id("inspector-title"),
            when(
                move || s.selected.get().is_none(),
                || label(res::str::select_service()).secondary(),
            ),
            when(
                move || s.selected.get().is_some(),
                move || {
                    column((
                        label(move || s.selected.get().map(|id| id.name).unwrap_or_default())
                            .font(Font::Headline)
                            .selectable()
                            .id("service-name"),
                        label(move || {
                            let Some(id) = s.selected.get() else {
                                return String::new();
                            };
                            res::str::identity(id.domain, id.interface.to_string(), id.service_type)
                                .format()
                        })
                        .selectable(),
                        when(
                            move || s.service().is_none(),
                            || label(res::str::resolving()).id("service-resolving"),
                        ),
                        when(
                            move || s.service().is_some(),
                            move || {
                                column((
                                    label(move || {
                                        s.service()
                                            .map(|v| {
                                                res::str::endpoint(v.hostname, v.port.to_string())
                                                    .format()
                                            })
                                            .unwrap_or_default()
                                    })
                                    .selectable()
                                    .id("service-endpoint"),
                                    section((label(move || {
                                        s.service()
                                            .map(|v| {
                                                v.addresses
                                                    .iter()
                                                    .map(ToString::to_string)
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            })
                                            .unwrap_or_default()
                                    })
                                    .monospace()
                                    .selectable()
                                    .id("service-addresses"),))
                                    .title(res::str::addresses()),
                                    section((label(move || {
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
                                                            res::str::txt_flag(k.clone()).format()
                                                        }
                                                        Some(v) => match std::str::from_utf8(v) {
                                                            Ok(text)
                                                                if !text
                                                                    .chars()
                                                                    .any(char::is_control) =>
                                                            {
                                                                format!("{k}={text}")
                                                            }
                                                            _ => format!(
                                                                "{k}=0x{}",
                                                                v.iter()
                                                                    .map(|b| format!("{b:02x}"))
                                                                    .collect::<String>()
                                                            ),
                                                        },
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            })
                                            .unwrap_or_default()
                                    })
                                    .monospace()
                                    .selectable()
                                    .id("service-txt"),))
                                    .title(res::str::txt_records()),
                                    section((
                                        label(res::str::probe_caption()).font(Font::Footnote),
                                        button(res::str::tcp_connect())
                                            .enabled(move || {
                                                !s.probing.get()
                                                    && s.service().is_some_and(|v| {
                                                        !v.addresses.is_empty()
                                                            && v.id.service_type.ends_with("._tcp")
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
                                        label(s.probe).selectable().id("probe-result"),
                                        when(
                                            move || s.probe_ok.get(),
                                            || label(res::str::connection_ok()).id("connection-ok"),
                                        ),
                                        when(
                                            move || {
                                                s.service().is_some_and(|v| {
                                                    v.addresses.iter().any(|a| a.is_ipv4())
                                                })
                                            },
                                            || {
                                                label(res::str::connection_ready())
                                                    .font(Font::Footnote)
                                                    .id("connection-ready")
                                            },
                                        ),
                                    ))
                                    .title(res::str::connections()),
                                ))
                                .spacing(12.0)
                            },
                        ),
                    ))
                    .spacing(12.0)
                },
            ),
            section((
                label(res::str::platform_notes()).font(Font::Footnote),
                label(move || {
                    s.log
                        .get()
                        .into_iter()
                        .rev()
                        .take(15)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .font(Font::Footnote)
                .selectable()
                .id("event-log"),
            ))
            .title(res::str::activity()),
        ))
        .spacing(16.0)
        .padding(16.0),
    )
}
pub fn publisher() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::publish())
                .font(Font::Title)
                .id("publisher-title"),
            label(res::str::publish_caption()).font(Font::Footnote),
            section((
                labeled(
                    res::str::instance_name(),
                    text_field(s.name).id("publish-name"),
                ),
                labeled(
                    res::str::service_type(),
                    text_field(s.publish_kind).id("publish-type"),
                ),
                labeled(
                    res::str::port(),
                    text_field(s.port)
                        .placeholder(res::str::port_hint())
                        .id("publish-port"),
                ),
                labeled(
                    res::str::txt_records(),
                    text_area(s.txt)
                        .min_lines(3)
                        .max_lines(6)
                        .spellcheck(false)
                        .id("publish-txt"),
                ),
                label(res::str::txt_hint()).font(Font::Footnote),
                row((
                    button(res::str::advertise())
                        .enabled(move || !s.publishing.get())
                        .action(move || s.publish())
                        .id("publish-start"),
                    button(res::str::withdraw())
                        .enabled(move || s.publishing.get())
                        .action(move || s.stop_publish())
                        .id("publish-stop"),
                ))
                .spacing(8.0),
                label(s.publish_status).selectable().id("publish-status"),
                when(
                    move || s.published.get(),
                    || label(res::str::advertisement_active()).id("advertisement-active"),
                ),
            ))
            .title(res::str::advertisement()),
        ))
        .spacing(16.0)
        .padding(16.0),
    )
}
