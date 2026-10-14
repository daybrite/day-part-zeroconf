// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! DNS presentation names returned by Windows. Kept independent of FFI for packet-name tests.
pub(crate) fn escape(s: &str) -> String {
    s.replace('\\', "\\092").replace('.', "\\046")
}
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit) {
                let n = (bytes[i + 1] - b'0') as u16 * 100
                    + (bytes[i + 2] - b'0') as u16 * 10
                    + (bytes[i + 3] - b'0') as u16;
                if n <= 255 {
                    out.push(n as u8);
                    i += 4;
                    continue;
                }
            }
            i += 1;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
pub(crate) fn instance(full: &str, kind: &str) -> Option<String> {
    let full = full.trim_end_matches('.');
    let suffix = format!(".{kind}.local");
    let split = full.len().checked_sub(suffix.len())?;
    if !full.get(split..)?.eq_ignore_ascii_case(&suffix) || split == 0 {
        return None;
    }
    Some(unescape(&full[..split]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_names_preserve_instance_case_and_decode_dns_escapes() {
        for full in [
            r"My\032Reader\046v2._HTTP._TCP.LOCAL.",
            r"My Reader\.v2._http._tcp.local",
        ] {
            assert_eq!(instance(full, "_http._tcp"), Some("My Reader.v2".into()));
        }
        let original = "Marc’s reader.\\library";
        assert_eq!(
            instance(
                &format!("{}._http._tcp.local", escape(original)),
                "_http._tcp"
            ),
            Some(original.into())
        );
        assert!(instance("foreign._http._tcp.local.evil", "_http._tcp").is_none());
        assert!(instance("foreign._ssh._tcp.local", "_http._tcp").is_none());
        assert!(instance("._http._tcp.local", "_http._tcp").is_none());
    }
}
