use day_part_zeroconf::{BrowseOptions, Event, browse};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kind = std::env::args().nth(1);
    let options = kind.map_or_else(BrowseOptions::all, BrowseOptions::service);
    let mut browser = browse(options)?;
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Some(e) = browser.recv_timeout(Duration::from_millis(250)) {
            println!("{e:?}");
            if matches!(e, Event::Error(_)) {
                break;
            }
        }
    }
    browser.stop();
    Ok(())
}
