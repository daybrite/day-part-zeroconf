// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day::prelude::*;
use day_piece_settings::prelude::*;
mod model;
mod network;
mod pages;
use model::Scene;
day::day_start!(options: window(), root);
day::resources!();
day::routes! { pub(crate) enum Section { Browse=>"browse", Publish=>"publish", Settings=>"settings" } }
pub fn window() -> day::WindowOptions {
    day::WindowOptions {
        locales: Some((res::locales::DEFAULT, res::locales::CATALOG)),
        title_fn: Some(|| res::str::app_title().format()),
        size: Size::new(1000.0, 760.0),
        ..Default::default()
    }
}
pub fn root() -> impl Piece {
    // Settings and About: appearance and language from Day's catalog, the app's name and
    // version from Day.toml. On desktop this adds the app menu's Settings… and About items and
    // their windows; on a phone it is the page behind the Settings section below.
    // https://daybrite.dev/docs/settings/
    let prefs = Preferences::new()
        .about(About::from_manifest().link(day::tr("day-about-website"), "https://daybrite.dev"))
        .group(general_group(res::locales::ALL))
        .install();
    Scene::scoped(move |s| {
        nav(s.section)
            .title(res::str::app_title())
            .content_list(pages::browser)
            .content_list_width(340.0)
            .content_list_for(|v: &Section| matches!(v, Section::Browse))
            .detail_visible(s.detail_open)
            .detail_title(move || {
                s.selected
                    .get()
                    .map_or_else(|| res::str::inspector().format(), |id| id.name)
            })
            .item_icon(
                Section::Browse,
                res::str::browse(),
                res::vectors::network,
                pages::inspector,
            )
            .item_icon(
                Section::Publish,
                res::str::publish(),
                res::vectors::publish,
                pages::publisher,
            )
            // Use the desktop's conventional Settings menu/window; without a menu bar,
            // keep Settings reachable as a navigation section.
            .items(
                {
                    let prefs = prefs.clone();
                    move || prefs.nav_keys(Section::Settings)
                },
                |v: &Section| item(*v, res::str::nav_settings()).icon(res::vectors::tab_settings),
            )
            .destination(move |_: &Section| prefs.page())
            .id("nav")
    })
}
