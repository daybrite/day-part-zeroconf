// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Opt-in tests against the actual OS daemon, never a simulated discovery backend.
//! Run: cargo test --test native -- --ignored --nocapture
use day_part_zeroconf::*;
use std::{
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
fn until(stage: &str, op: &mut Operation, mut predicate: impl FnMut(Event) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(event) = op.recv_timeout(Duration::from_millis(100)) {
            eprintln!("{stage}: {event:?}");
            if let Event::Error(e) = &event {
                panic!("{stage}: native discovery: {e}")
            }
            if predicate(event) {
                return;
            }
        }
    }
    panic!("{stage}: native event timed out")
}
#[test]
#[ignore = "requires local network permission and the native DNS-SD daemon"]
fn advertise_discover_connect_and_withdraw() {
    let listener = TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let name = format!("Day fixture {}", std::process::id());
    let kind = "_dayzc-test._tcp";
    let mut browser = browse(BrowseOptions::service(kind)).unwrap();
    let mut publisher = advertise(Advertisement {
        name: name.clone(),
        service_type: kind.into(),
        port,
        txt: TxtRecord(vec![
            ("path".into(), Some(b"/fixture".to_vec())),
            ("empty".into(), Some(vec![])),
        ]),
        interface: 0,
    })
    .unwrap();
    let mut actual = name;
    until("registration", &mut publisher, |e| {
        if let Event::Registered(id) = e {
            actual = id.name;
            true
        } else {
            false
        }
    });
    let mut endpoint = None;
    until("discovery and IPv4 resolution", &mut browser, |e| {
        if let Event::Resolved(s) = e
            && s.id.name == actual
            && s.addresses.iter().any(|a| a.is_ipv4())
        {
            endpoint = Some(s);
            true
        } else {
            false
        }
    });
    let service = endpoint.unwrap();
    assert_eq!(service.port, port);
    assert_eq!(service.txt.get("path"), Some(&b"/fixture"[..]));
    assert_eq!(service.txt.get("empty"), Some(&b""[..]));
    let socket = service.addresses.iter().find(|a| a.is_ipv4()).unwrap();
    let connection = TcpStream::connect_timeout(socket, Duration::from_secs(3)).unwrap();
    drop(connection);
    publisher.stop();
    until(
        "withdrawal",
        &mut browser,
        |e| matches!(e,Event::Removed(id) if id.name==actual),
    );
    browser.stop();
}
#[test]
#[ignore = "requires native all-types browsing permission"]
// DnsServiceRegister does not publish the _services._dns-sd._udp PTR. Testing
// Windows enumeration needs a peer responder; its own registration is no oracle.
#[cfg(not(target_os = "windows"))]
fn enumerate_service_types_and_cancel() {
    let listener = TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let mut publisher = advertise(Advertisement {
        name: format!("Day enumeration fixture {}", std::process::id()),
        service_type: "_dayzc-enum._tcp".into(),
        port: listener.local_addr().unwrap().port(),
        txt: TxtRecord::default(),
        interface: 0,
    })
    .unwrap();
    until("enumeration registration", &mut publisher, |e| {
        matches!(e, Event::Registered(_))
    });
    let mut browser = browse(BrowseOptions::all()).unwrap();
    until(
        "service-type enumeration",
        &mut browser,
        |e| matches!(e,Event::ServiceType(t) if t=="_dayzc-enum._tcp"),
    );
    browser.cancellation().cancel();
    until("cancellation", &mut browser, |e| {
        matches!(e, Event::Stopped)
    });
    publisher.stop();
}

#[test]
#[ignore = "requires the native DNS-SD daemon"]
fn targeted_browse_cancels() {
    let mut browser = browse(BrowseOptions::service("_dayzc-test._tcp")).unwrap();
    until("browse start", &mut browser, |e| {
        matches!(e, Event::Started)
    });
    browser.cancellation().cancel();
    until("targeted cancellation", &mut browser, |e| {
        matches!(e, Event::Stopped)
    });
}
