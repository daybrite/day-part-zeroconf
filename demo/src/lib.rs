// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day::prelude::*;
mod model;
mod network;
mod pages;
use model::Scene;
day::day_start!(options: window(), root);
day::resources!();
const THEME_KEY: &str = "explorer.theme";
const LOCALE_KEY: &str = "explorer.locale";
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
    day_piece_settings::apply_startup(THEME_KEY, LOCALE_KEY);
    Scene::scoped(|s| {
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
            .item_icon(
                Section::Settings,
                res::str::nav_settings(),
                res::vectors::tab_settings,
                pages::settings::settings_page,
            )
            .id("nav")
    })
}
