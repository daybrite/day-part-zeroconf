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
    res::window_options().size(1000.0, 760.0)
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
            .content_list(
                ContentList::new(pages::browser)
                    .width(340.0)
                    .only_for(|v: &Section| matches!(v, Section::Browse))
                    .detail_visible(s.detail_open)
                    .detail_title(move || {
                        s.selected
                            .get()
                            .map_or_else(|| res::str::inspector().format(), |id| id.name)
                    }),
            )
            .items((
                item(Section::Browse, res::str::browse())
                    .icon(res::vectors::network)
                    .page(pages::inspector),
                item(Section::Publish, res::str::publish())
                    .icon(res::vectors::publish)
                    .page(pages::publisher),
                // Use the desktop's conventional Settings menu/window; without a menu bar,
                // keep Settings reachable as a navigation section.
                prefs
                    .nav_item(Section::Settings)
                    .map(|i| i.icon(res::vectors::tab_settings)),
            ))
            .id(day::id!(nav))
    })
}
