//! Validation of user-supplied LLM endpoints (PLANO_VAD.md §4.28).
//!
//! A custom `base_url` is the only place where the user picks an arbitrary network
//! destination for the app to talk to, so it is parsed with `url::Url` and checked
//! before it is saved or used — never trusted as a string.

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};
use vad_core::VadError;

/// Official OpenAI API root.
pub const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// Hostnames of cloud metadata services (credentials theft target).
const METADATA_HOSTS: [&str; 4] = [
    "metadata.google.internal",
    "metadata",
    "instance-data",
    "metadata.azure.com",
];

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    // 169.254.0.0/16 is link-local and holds 169.254.169.254 (AWS/GCP/Azure metadata).
    ip.is_link_local() || ip.octets() == [100, 100, 100, 200] // Alibaba Cloud metadata
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    let seg = ip.segments();
    // fe80::/10 link-local, fd00:ec2::254 (AWS IMDS over IPv6).
    (seg[0] & 0xffc0) == 0xfe80 || ip == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254)
}

/// Parses and validates a base URL. Returns it normalised (no trailing slash, no
/// query or fragment).
///
/// Rejects: unparsable input, schemes other than http/https, missing host, embedded
/// credentials (`user:pass@`, which would end up in `config.toml`), and cloud metadata
/// addresses.
pub fn validate_base_url(raw: &str) -> Result<Url, VadError> {
    let raw = raw.trim();
    let mut url = Url::parse(raw)
        .map_err(|e| VadError::Llm(format!("base_url inválido ({e}): «{raw}»")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(VadError::Llm(format!(
            "base_url com esquema «{}» não é permitido — usa http ou https",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(VadError::Llm(
            "base_url não pode conter credenciais — a chave vai nas Definições, não no URL".into(),
        ));
    }
    match url.host() {
        None => return Err(VadError::Llm("base_url sem anfitrião".into())),
        Some(Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            if METADATA_HOSTS.contains(&d.as_str()) {
                return Err(VadError::Llm(format!("endereço de metadados cloud bloqueado: {d}")));
            }
        }
        Some(Host::Ipv4(ip)) if is_blocked_v4(ip) => {
            return Err(VadError::Llm(format!("endereço de metadados cloud bloqueado: {ip}")));
        }
        Some(Host::Ipv6(ip)) if is_blocked_v6(ip) => {
            return Err(VadError::Llm(format!("endereço de metadados cloud bloqueado: {ip}")));
        }
        Some(_) => {}
    }
    url.set_query(None);
    url.set_fragment(None);
    let trimmed = url.path().trim_end_matches('/').to_string();
    url.set_path(&trimmed);
    Ok(url)
}

/// True if the host is this very machine, so nothing leaves the PC (`localhost`,
/// `127.0.0.0/8`, `::1`). A LAN address is **not** local: the text does leave the PC.
pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v| v.is_loopback()),
        None => false,
    }
}

/// True for `https://api.openai.com/...`. Anything else is a custom endpoint and goes
/// through the same quality gate as `LocalQwen` (§4.1).
pub fn is_official_openai(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(url.host(), Some(Host::Domain(d)) if d.eq_ignore_ascii_case("api.openai.com"))
}

/// Convenience for string configs: `None` if invalid.
pub fn is_loopback_str(raw: &str) -> bool {
    validate_base_url(raw).map(|u| is_loopback(&u)).unwrap_or(false)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_official_and_local_endpoints() {
        let u = validate_base_url(" https://api.openai.com/v1/ ").unwrap();
        assert_eq!(u.as_str(), "https://api.openai.com/v1");
        assert!(is_official_openai(&u));
        let o = validate_base_url("http://localhost:11434/v1").unwrap();
        assert!(is_loopback(&o) && !is_official_openai(&o));
    }

    #[test]
    fn rejects_bad_schemes_and_garbage() {
        for bad in ["file:///etc/passwd", "ftp://x.org/v1", "javascript:alert(1)", "api.openai.com/v1", "", "http://"] {
            assert!(validate_base_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn rejects_metadata_addresses_in_every_spelling() {
        for bad in [
            "http://169.254.169.254/latest/meta-data",
            "http://169.254.0.1/",
            "http://2852039166/",          // decimal form of 169.254.169.254
            "http://0xa9fea9fe/",          // hex form
            "http://[::ffff:169.254.169.254]/",
            "http://[fd00:ec2::254]/",
            "http://[fe80::1]/",
            "http://metadata.google.internal./computeMetadata/v1",
            "http://METADATA.GOOGLE.INTERNAL/",
            "http://100.100.100.200/",
        ] {
            assert!(validate_base_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn rejects_embedded_credentials_and_strips_query() {
        assert!(validate_base_url("https://user:pw@example.com/v1").is_err());
        let u = validate_base_url("https://example.com/v1/?key=abc#frag").unwrap();
        assert_eq!(u.as_str(), "https://example.com/v1");
    }

    #[test]
    fn lan_and_lookalike_hosts_are_not_loopback() {
        for s in ["http://192.168.1.10:11434/v1", "http://localhost.evil.com/v1", "http://127.0.0.1.evil.com/v1", "https://openrouter.ai/api/v1"] {
            assert!(!is_loopback(&validate_base_url(s).unwrap()), "{s}");
        }
        assert!(is_loopback(&validate_base_url("http://127.0.0.1:8080").unwrap()));
        assert!(is_loopback(&validate_base_url("http://[::1]:8080").unwrap()));
        assert!(!is_official_openai(&validate_base_url("https://api.openai.com.evil.com/v1").unwrap()));
        assert!(!is_official_openai(&validate_base_url("http://api.openai.com/v1").unwrap()));
    }
}
