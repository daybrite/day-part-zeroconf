// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::{Section, network, res};
use day::prelude::*;
use day_part_zeroconf::{self as zc, Cancellation, Event, Service, ServiceId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Copy)]
pub(crate) struct Scene {
    pub section: Signal<Section>,
    pub detail_open: Signal<bool>,
    pub selected: Signal<Option<ServiceId>>,
    pub services: Signal<BTreeMap<ServiceId, Option<Service>>>,
    pub discovered: Signal<BTreeSet<ServiceId>>,
    pub filter: Signal<String>,
    pub kind: Signal<String>,
    pub browsing: Signal<bool>,
    pub status: Signal<String>,
    pub browse_cancel: Signal<Option<Cancellation>>,
    pub generation: Signal<u64>,
    pub name: Signal<String>,
    pub publish_kind: Signal<String>,
    pub port: Signal<String>,
    pub txt: Signal<String>,
    pub publish_cancel: Signal<Option<Cancellation>>,
    pub publish_generation: Signal<u64>,
    pub publishing: Signal<bool>,
    pub publish_status: Signal<String>,
    pub server: Signal<Option<Arc<network::Server>>>,
    pub probe: Signal<String>,
    pub probing: Signal<bool>,
    pub probe_ok: Signal<bool>,
    pub published: Signal<bool>,
    pub log: Signal<Vec<String>>,
}
impl Ambient for Scene {
    fn create() -> Self {
        Self {
            section: Signal::new(Section::Browse),
            detail_open: Signal::new(false),
            selected: Signal::new(None),
            services: Signal::new(BTreeMap::new()),
            discovered: Signal::new(BTreeSet::new()),
            filter: Signal::new(String::new()),
            kind: Signal::new(String::new()),
            browsing: Signal::new(false),
            status: Signal::new(res::str::ready().format()),
            browse_cancel: Signal::new(None),
            generation: Signal::new(0),
            name: Signal::new(res::str::sample_name().format()),
            publish_kind: Signal::new("_dayzc._tcp".into()),
            port: Signal::new(String::new()),
            txt: Signal::new("path=/\nversion=1".into()),
            publish_cancel: Signal::new(None),
            publish_generation: Signal::new(0),
            publishing: Signal::new(false),
            publish_status: Signal::new(res::str::not_publishing().format()),
            server: Signal::new(None),
            probe: Signal::new(String::new()),
            probing: Signal::new(false),
            probe_ok: Signal::new(false),
            published: Signal::new(false),
            log: Signal::new(vec![]),
        }
    }
}
pub(crate) fn error(e: &zc::Error) -> String {
    match e {
        zc::Error::PermissionDenied => res::str::permission_denied().format(),
        zc::Error::Unsupported => res::str::unsupported().format(),
        zc::Error::InvalidInput(_) => res::str::invalid_input().format(),
        zc::Error::Overflow => res::str::overflow().format(),
        zc::Error::Native { code, .. } => res::str::native_error(code.to_string()).format(),
        zc::Error::Cancelled => res::str::stopped().format(),
        _ => res::str::unavailable().format(),
    }
}
impl Scene {
    pub fn record(self, text: String) {
        self.log.update(|log| {
            log.push(text);
            if log.len() > 100 {
                log.remove(0);
            }
        });
    }
    pub fn stop_browse(self) {
        if let Some(c) = self.browse_cancel.get_untracked() {
            c.cancel()
        }
        self.browse_cancel.set(None);
        self.generation.update(|n| *n += 1);
        self.browsing.set(false);
        self.status.set(res::str::stopped().format());
    }
    pub fn browse(self) {
        self.stop_browse();
        self.services.set(BTreeMap::new());
        self.discovered.set(BTreeSet::new());
        self.selected.set(None);
        self.detail_open.set(false);
        let kind = self.kind.get_untracked().trim().to_owned();
        let options = if kind.is_empty() {
            zc::BrowseOptions::all()
        } else {
            zc::BrowseOptions::service(kind)
        };
        let generation = self.generation.get_untracked();
        self.browsing.set(true);
        self.status.set(res::str::starting().format());
        day::task(async move {
            if !permission().await {
                self.browsing.set(false);
                self.status.set(res::str::permission_denied().format());
                return;
            }
            if generation != self.generation.get_untracked() {
                return;
            }
            match zc::browse(options) {
                Err(e) => {
                    self.browsing.set(false);
                    self.status.set(error(&e));
                }
                Ok(mut op) => {
                    self.browse_cancel.set(Some(op.cancellation()));
                    while let Some(event) = op.next_event().await {
                        if generation != self.generation.get_untracked() {
                            break;
                        }
                        match event {
                            Event::Started => self.status.set(res::str::browsing().format()),
                            Event::Found(id) => {
                                self.discovered.update(|ids| {
                                    ids.insert(id.clone());
                                });
                                self.services.update(|m| {
                                    m.entry(id.clone()).or_default();
                                });
                                self.record(res::str::found(id.name).format());
                            }
                            Event::Resolved(service) => {
                                self.services.update(|m| {
                                    if m.contains_key(&service.id) {
                                        m.insert(service.id.clone(), Some(service));
                                    }
                                });
                            }
                            Event::Removed(id) => {
                                self.discovered.update(|ids| {
                                    ids.remove(&id);
                                });
                                self.services.update(|m| {
                                    m.remove(&id);
                                });
                                self.record(res::str::removed(id.name.clone()).format());
                                if self.selected.get_untracked().as_ref() == Some(&id) {
                                    self.selected.set(None);
                                    self.detail_open.set(false);
                                }
                            }
                            Event::Error(e) => {
                                let text = error(&e);
                                self.status.set(text.clone());
                                self.record(text);
                            }
                            Event::ServiceType(t) => {
                                let text = res::str::service_type_found(t).format();
                                self.status.set(text.clone());
                                self.record(text);
                            }
                            _ => {}
                        }
                    }
                    if generation == self.generation.get_untracked() {
                        self.browsing.set(false);
                        self.browse_cancel.set(None);
                    }
                }
            }
        });
    }
    pub fn rows(self) -> Vec<ServiceId> {
        let q = self.filter.get().to_lowercase();
        self.discovered
            .get()
            .into_iter()
            .filter(|id| {
                q.is_empty()
                    || id.name.to_lowercase().contains(&q)
                    || id.service_type.contains(&q)
                    || self.services.with(|m| {
                        m.get(id)
                            .and_then(|s| s.as_ref())
                            .is_some_and(|s| s.hostname.to_lowercase().contains(&q))
                    })
            })
            .collect()
    }
    pub fn service(self) -> Option<Service> {
        self.selected
            .get()
            .and_then(|id| self.services.with(|m| m.get(&id).cloned().flatten()))
    }
    pub fn select(self, id: ServiceId) {
        self.selected.set(Some(id));
        self.probe.set(String::new());
        self.probe_ok.set(false);
        self.detail_open.set(true);
    }
    pub fn stop_publish(self) {
        if let Some(c) = self.publish_cancel.get_untracked() {
            c.cancel()
        }
        self.publish_cancel.set(None);
        self.server.set(None);
        self.publish_generation.update(|n| *n += 1);
        self.publishing.set(false);
        self.published.set(false);
        self.publish_status.set(res::str::not_publishing().format());
    }
    pub fn publish(self) {
        self.stop_publish();
        let name = self.name.get_untracked();
        let kind = self.publish_kind.get_untracked();
        let txt = match network::parse_txt(&self.txt.get_untracked()) {
            Ok(t) => t,
            Err(e) => {
                self.publish_status.set(error(&e));
                return;
            }
        };
        let port_text = self.port.get_untracked();
        let port = if port_text.trim().is_empty() {
            None
        } else {
            match port_text.trim().parse::<u16>() {
                Ok(n) if n != 0 => Some(n),
                _ => {
                    self.publish_status.set(res::str::invalid_input().format());
                    return;
                }
            }
        };
        if port.is_none() && !kind.ends_with("._tcp") {
            self.publish_status
                .set(res::str::auto_server_tcp().format());
            return;
        }
        let body = res::str::hello_body().format();
        let generation = self.publish_generation.get_untracked();
        self.publishing.set(true);
        self.publish_status.set(res::str::starting().format());
        day::task(async move {
            if !permission().await {
                self.publishing.set(false);
                self.publish_status
                    .set(res::str::permission_denied().format());
                return;
            }
            if generation != self.publish_generation.get_untracked() {
                return;
            }
            let port = match port {
                Some(p) => p,
                None => match network::Server::start(body) {
                    Ok(server) => {
                        let port = server.port;
                        self.server.set(Some(Arc::new(server)));
                        port
                    }
                    Err(_) => {
                        self.publishing.set(false);
                        self.publish_status.set(res::str::server_failed().format());
                        return;
                    }
                },
            };
            match zc::advertise(zc::Advertisement {
                name,
                service_type: kind,
                port,
                txt,
                interface: 0,
            }) {
                Err(e) => {
                    self.server.set(None);
                    self.publishing.set(false);
                    self.publish_status.set(error(&e));
                }
                Ok(mut op) => {
                    self.publish_cancel.set(Some(op.cancellation()));
                    while let Some(event) = op.next_event().await {
                        if generation != self.publish_generation.get_untracked() {
                            break;
                        }
                        match event {
                            Event::Registered(id) => {
                                self.published.set(true);
                                self.publish_status.set(
                                    res::str::advertised(res::str::Advertised {
                                        name: id.name,
                                        port: port.to_string(),
                                    })
                                    .format(),
                                );
                            }
                            Event::Error(e) => {
                                self.publish_status.set(error(&e));
                                break;
                            }
                            _ => {}
                        }
                    }
                    if generation == self.publish_generation.get_untracked() {
                        self.publishing.set(false);
                        self.server.set(None);
                        self.publish_cancel.set(None);
                    }
                }
            }
        });
    }
    pub fn connect(self, http: bool) {
        let Some(service) = self.service() else {
            return;
        };
        let id = service.id.clone();
        self.probing.set(true);
        self.probe_ok.set(false);
        self.probe.set(res::str::connecting().format());
        day::task(async move {
            let result = day::spawn_blocking(move || network::probe(&service, http)).await;
            let success = result.as_ref().is_ok_and(|r| r.is_ok());
            let message = match result {
                Ok(Ok(network::Probe::Tcp(address))) => {
                    res::str::tcp_success(address.to_string()).format()
                }
                Ok(Ok(network::Probe::Http(response))) => res::str::http_success(response).format(),
                _ => res::str::connection_failed().format(),
            };
            self.record(message.clone());
            if self.selected.get_untracked().as_ref() == Some(&id) {
                self.probe.set(message);
                self.probe_ok.set(success);
            }
            self.probing.set(false);
        });
    }
}
async fn permission() -> bool {
    #[cfg(target_os = "android")]
    {
        use day_part_permissions::{Permission, Status};
        let p = Permission::Raw("android.permission.ACCESS_LOCAL_NETWORK");
        if zc::requires_explicit_permission()
            && matches!(
                day_part_permissions::status(p),
                Status::Denied | Status::Prompt
            )
        {
            return day_part_permissions::request_future(p).await == Status::Granted;
        }
    }
    true
}
