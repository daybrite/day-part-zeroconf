// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use super::*;
pub(crate) fn decode(bytes: &[u8]) -> Result<Event> {
    struct Reader<'a>(&'a [u8]);
    impl Reader<'_> {
        fn u32(&mut self) -> Result<u32> {
            if self.0.len() < 4 {
                return Err(Error::InvalidInput("NSD frame".into()));
            }
            let v = u32::from_be_bytes(self.0[..4].try_into().unwrap());
            self.0 = &self.0[4..];
            Ok(v)
        }
        fn bytes(&mut self) -> Result<&[u8]> {
            let n = self.u32()? as usize;
            if n > self.0.len() {
                return Err(Error::InvalidInput("NSD frame".into()));
            }
            let (v, tail) = self.0.split_at(n);
            self.0 = tail;
            Ok(v)
        }
        fn str(&mut self) -> Result<String> {
            String::from_utf8(self.bytes()?.to_vec())
                .map_err(|_| Error::InvalidInput("NSD UTF-8".into()))
        }
    }
    let mut r = Reader(bytes);
    let tag = r.u32()?;
    if tag == 5 {
        return Ok(Event::ServiceType(validate_type(&r.str()?)?));
    }
    if tag == 6 {
        let code = r.u32()? as i32;
        let message = r.str()?;
        return Ok(Event::Error(
            if code == 201 || code == -65570 || code == (-1) {
                Error::PermissionDenied
            } else {
                Error::Native {
                    code: code.into(),
                    message,
                }
            },
        ));
    }
    let id = ServiceId {
        name: r.str()?,
        service_type: validate_type(&r.str()?)?,
        domain: r.str()?,
        interface: 0,
    };
    match tag {
        1 => Ok(Event::Found(id)),
        2 => Ok(Event::Removed(id)),
        4 => Ok(Event::Registered(id)),
        3 => {
            let hostname = r.str()?;
            let port =
                u16::try_from(r.u32()?).map_err(|_| Error::InvalidInput("NSD port".into()))?;
            let n = r.u32()?;
            if n > 64 {
                return Err(Error::Overflow);
            }
            let mut addresses = vec![];
            for _ in 0..n {
                let address = r.str()?;
                let text = if address.contains(':') {
                    format!("[{address}]:{port}")
                } else {
                    format!("{address}:{port}")
                };
                if let Ok(a) = text.parse() {
                    addresses.push(a)
                }
            }
            let txt = TxtRecord::decode(r.bytes()?)?;
            Ok(Event::Resolved(Service {
                id,
                hostname,
                port,
                addresses,
                txt,
            }))
        }
        _ => Err(Error::InvalidInput("NSD event tag".into())),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn number(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_be_bytes())
    }
    fn field(b: &mut Vec<u8>, s: &[u8]) {
        number(b, s.len() as u32);
        b.extend(s)
    }
    #[test]
    fn malformed_frames_rejected() {
        for bytes in [
            vec![],
            vec![0, 0, 0, 9],
            vec![0, 0, 0, 5, 255, 255, 255, 255],
        ] {
            assert!(decode(&bytes).is_err())
        }
    }
    #[test]
    fn mobile_addresses_and_binary_txt() {
        let mut b = vec![];
        number(&mut b, 3);
        for s in ["Fixture", "_dayzc._tcp", "local.", "fixture.local."] {
            field(&mut b, s.as_bytes())
        }
        number(&mut b, 1234);
        number(&mut b, 2);
        field(&mut b, b"192.0.2.1");
        field(&mut b, b"fe80::1%7");
        field(&mut b, &[4, b'x', b'=', 0, 255]);
        let Event::Resolved(s) = decode(&b).unwrap() else {
            panic!("service expected")
        };
        assert_eq!(
            s.addresses[1],
            "[fe80::1%7]:1234".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(s.txt.get("x"), Some(&[0, 255][..]));
    }
}
