use core::fmt::Write as _;

use heapless::String;

use crate::net_types::{API_VERSION, HOSTNAME_PREFIX, SERVICE_ROLE, SERVICE_TYPE};

pub const MDNS_MULTICAST_V4: [u8; 4] = [224, 0, 0, 251];
pub const MDNS_PORT: u16 = 5353;
pub const MDNS_RESPONSE_TTL_SECS: u32 = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub mac: [u8; 6],
    pub short_id: String<6>,
    pub device_id: String<32>,
    pub hostname: String<32>,
    pub hostname_fqdn: String<48>,
    pub service_instance: String<96>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdnsQuery {
    pub name: String<96>,
    pub qtype: u16,
    pub unicast_response: bool,
}

pub fn short_id_from_mac(mac: [u8; 6]) -> String<6> {
    let mut out: String<6> = String::new();
    for byte in mac.iter().skip(3) {
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

pub fn hostname_from_short_id(short_id: &str) -> String<32> {
    let mut out: String<32> = String::new();
    let _ = out.push_str(HOSTNAME_PREFIX);
    for ch in short_id.chars() {
        if ch.is_ascii_hexdigit() {
            let _ = out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

pub fn fqdn_from_hostname(hostname: &str) -> String<48> {
    let mut out: String<48> = String::new();
    let _ = out.push_str(hostname);
    let _ = out.push_str(".local");
    out
}

pub fn service_instance_from_hostname(hostname: &str) -> String<96> {
    let mut out: String<96> = String::new();
    let _ = out.push_str(hostname);
    let _ = out.push('.');
    let _ = out.push_str(SERVICE_TYPE);
    out
}

pub fn derive_device_identity(mac: [u8; 6]) -> DeviceIdentity {
    let short_id = short_id_from_mac(mac);
    let hostname = hostname_from_short_id(short_id.as_str());
    let hostname_fqdn = fqdn_from_hostname(hostname.as_str());
    let service_instance = service_instance_from_hostname(hostname.as_str());
    DeviceIdentity {
        mac,
        short_id,
        device_id: hostname.clone(),
        hostname,
        hostname_fqdn,
        service_instance,
    }
}

pub fn parse_query(packet: &[u8]) -> Option<MdnsQuery> {
    if packet.len() < 12 {
        return None;
    }

    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    if qdcount == 0 {
        return None;
    }

    let mut offset = 12;
    let name = decode_name(packet, &mut offset)?;
    if offset + 4 > packet.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
    let qclass = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]);
    Some(MdnsQuery {
        name,
        qtype,
        unicast_response: (qclass & 0x8000) != 0,
    })
}

pub fn query_matches(query: &MdnsQuery, identity: &DeviceIdentity) -> bool {
    let matches_name = query
        .name
        .eq_ignore_ascii_case(identity.hostname_fqdn.as_str())
        || query.name.eq_ignore_ascii_case(SERVICE_TYPE)
        || query
            .name
            .eq_ignore_ascii_case(identity.service_instance.as_str());
    if !matches_name {
        return false;
    }
    matches!(query.qtype, 1 | 12 | 16 | 33 | 255)
}

pub fn build_discovery_response(
    buf: &mut [u8],
    identity: &DeviceIdentity,
    ipv4: [u8; 4],
    port: u16,
    include_question: Option<&MdnsQuery>,
) -> Option<usize> {
    if buf.len() < 12 {
        return None;
    }

    buf[0] = 0;
    buf[1] = 0;
    buf[2] = 0x84;
    buf[3] = 0x00;

    let qdcount = if include_question.is_some() {
        1u16
    } else {
        0u16
    };
    buf[4..6].copy_from_slice(&qdcount.to_be_bytes());
    buf[6..8].copy_from_slice(&4u16.to_be_bytes());
    buf[8..10].copy_from_slice(&0u16.to_be_bytes());
    buf[10..12].copy_from_slice(&0u16.to_be_bytes());

    let mut offset = 12;

    if let Some(query) = include_question {
        offset = encode_name(buf, offset, query.name.as_str())?;
        offset = write_u16(buf, offset, query.qtype)?;
        offset = write_u16(buf, offset, 0x0001)?;
    }

    offset = encode_name(buf, offset, SERVICE_TYPE)?;
    offset = write_u16(buf, offset, 12)?;
    offset = write_u16(buf, offset, 0x8001)?;
    offset = write_u32(buf, offset, MDNS_RESPONSE_TTL_SECS)?;
    let rdlen_offset = offset;
    offset = write_u16(buf, offset, 0)?;
    let ptr_data_start = offset;
    offset = encode_name(buf, offset, identity.service_instance.as_str())?;
    set_rdlength(buf, rdlen_offset, offset - ptr_data_start)?;

    offset = encode_name(buf, offset, identity.service_instance.as_str())?;
    offset = write_u16(buf, offset, 33)?;
    offset = write_u16(buf, offset, 0x8001)?;
    offset = write_u32(buf, offset, MDNS_RESPONSE_TTL_SECS)?;
    let rdlen_offset = offset;
    offset = write_u16(buf, offset, 0)?;
    let srv_data_start = offset;
    offset = write_u16(buf, offset, 0)?; // priority
    offset = write_u16(buf, offset, 0)?; // weight
    offset = write_u16(buf, offset, port)?;
    offset = encode_name(buf, offset, identity.hostname_fqdn.as_str())?;
    set_rdlength(buf, rdlen_offset, offset - srv_data_start)?;

    offset = encode_name(buf, offset, identity.service_instance.as_str())?;
    offset = write_u16(buf, offset, 16)?;
    offset = write_u16(buf, offset, 0x8001)?;
    offset = write_u32(buf, offset, MDNS_RESPONSE_TTL_SECS)?;
    let rdlen_offset = offset;
    offset = write_u16(buf, offset, 0)?;
    let txt_data_start = offset;
    offset = encode_txt_entry(
        buf,
        offset,
        txt_entry("device_id", identity.device_id.as_str()).as_str(),
    )?;
    offset = encode_txt_entry(buf, offset, txt_entry("api_version", API_VERSION).as_str())?;
    offset = encode_txt_entry(buf, offset, txt_entry("role", SERVICE_ROLE).as_str())?;
    set_rdlength(buf, rdlen_offset, offset - txt_data_start)?;

    offset = encode_name(buf, offset, identity.hostname_fqdn.as_str())?;
    offset = write_u16(buf, offset, 1)?;
    offset = write_u16(buf, offset, 0x8001)?;
    offset = write_u32(buf, offset, MDNS_RESPONSE_TTL_SECS)?;
    offset = write_u16(buf, offset, 4)?;
    if offset + 4 > buf.len() {
        return None;
    }
    buf[offset..offset + 4].copy_from_slice(&ipv4);
    offset += 4;

    Some(offset)
}

fn txt_entry(key: &str, value: &str) -> String<64> {
    let mut out = String::<64>::new();
    let _ = out.push_str(key);
    let _ = out.push('=');
    let _ = out.push_str(value);
    out
}

fn encode_txt_entry(buf: &mut [u8], mut offset: usize, entry: &str) -> Option<usize> {
    let bytes = entry.as_bytes();
    if bytes.len() > u8::MAX as usize || offset + 1 + bytes.len() > buf.len() {
        return None;
    }
    buf[offset] = bytes.len() as u8;
    offset += 1;
    buf[offset..offset + bytes.len()].copy_from_slice(bytes);
    Some(offset + bytes.len())
}

fn set_rdlength(buf: &mut [u8], offset: usize, len: usize) -> Option<()> {
    if len > u16::MAX as usize || offset + 2 > buf.len() {
        return None;
    }
    buf[offset..offset + 2].copy_from_slice(&(len as u16).to_be_bytes());
    Some(())
}

fn write_u16(buf: &mut [u8], offset: usize, value: u16) -> Option<usize> {
    if offset + 2 > buf.len() {
        return None;
    }
    buf[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    Some(offset + 2)
}

fn write_u32(buf: &mut [u8], offset: usize, value: u32) -> Option<usize> {
    if offset + 4 > buf.len() {
        return None;
    }
    buf[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    Some(offset + 4)
}

fn encode_name(buf: &mut [u8], mut offset: usize, name: &str) -> Option<usize> {
    if name.is_empty() {
        if offset >= buf.len() {
            return None;
        }
        buf[offset] = 0;
        return Some(offset + 1);
    }

    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        if label.len() > 63 || offset + 1 + label.len() > buf.len() {
            return None;
        }
        buf[offset] = label.len() as u8;
        offset += 1;
        buf[offset..offset + label.len()].copy_from_slice(label.as_bytes());
        offset += label.len();
    }

    if offset >= buf.len() {
        return None;
    }
    buf[offset] = 0;
    Some(offset + 1)
}

fn decode_name(packet: &[u8], offset: &mut usize) -> Option<String<96>> {
    let mut out = String::<96>::new();
    let mut cursor = *offset;
    let mut jumped = false;
    let mut jump_end = 0usize;
    let mut seen = 0u8;

    loop {
        if cursor >= packet.len() || seen > 16 {
            return None;
        }
        let len = packet[cursor];
        if len & 0xC0 == 0xC0 {
            if cursor + 1 >= packet.len() {
                return None;
            }
            let ptr = (((len as u16 & 0x3F) << 8) | packet[cursor + 1] as u16) as usize;
            if !jumped {
                jump_end = cursor + 2;
            }
            cursor = ptr;
            jumped = true;
            seen = seen.saturating_add(1);
            continue;
        }
        cursor += 1;
        if len == 0 {
            *offset = if jumped { jump_end } else { cursor };
            break;
        }
        let end = cursor + len as usize;
        if end > packet.len() {
            return None;
        }
        if !out.is_empty() {
            let _ = out.push('.');
        }
        for &byte in &packet[cursor..end] {
            let _ = out.push(byte as char);
        }
        cursor = end;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        build_discovery_response, derive_device_identity, parse_query, query_matches, MdnsQuery,
        SERVICE_TYPE,
    };

    #[test]
    fn derive_identity_uses_last_three_mac_bytes() {
        let identity = derive_device_identity([0x30, 0xae, 0xa4, 0x12, 0x34, 0x56]);
        assert_eq!(identity.short_id.as_str(), "123456");
        assert_eq!(identity.hostname.as_str(), "mains-aegis-123456");
        assert_eq!(identity.hostname_fqdn.as_str(), "mains-aegis-123456.local");
    }

    #[test]
    fn parses_basic_mdns_query() {
        let mut query = [0u8; 128];
        query[4] = 0;
        query[5] = 1;
        let mut offset = 12;
        for part in ["_mains-aegis-ups", "_tcp", "local"] {
            query[offset] = part.len() as u8;
            offset += 1;
            query[offset..offset + part.len()].copy_from_slice(part.as_bytes());
            offset += part.len();
        }
        query[offset] = 0;
        offset += 1;
        query[offset..offset + 2].copy_from_slice(&12u16.to_be_bytes());
        query[offset + 2..offset + 4].copy_from_slice(&1u16.to_be_bytes());

        let parsed = parse_query(&query[..offset + 4]).expect("query");
        assert_eq!(parsed.name.as_str(), SERVICE_TYPE);
        assert_eq!(parsed.qtype, 12);
    }

    #[test]
    fn discovery_response_contains_dns_sd_records() {
        let identity = derive_device_identity([0x30, 0xae, 0xa4, 0x12, 0x34, 0x56]);
        let mut query_name = heapless::String::<96>::new();
        let _ = query_name.push_str(SERVICE_TYPE);
        let query = MdnsQuery {
            name: query_name,
            qtype: 12,
            unicast_response: false,
        };
        assert!(query_matches(&query, &identity));

        let mut buf = [0u8; 512];
        let len =
            build_discovery_response(&mut buf, &identity, [192, 168, 31, 15], 80, Some(&query))
                .expect("response");
        let encoded = &buf[..len];
        let text = String::from_utf8_lossy(encoded);
        assert!(text.contains("_mains-aegis-ups"));
        assert!(text.contains("device_id=mains-aegis-123456"));
        assert!(text.contains("api_version=v1"));
    }
}
