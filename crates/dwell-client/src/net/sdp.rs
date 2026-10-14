//! The ICE-lite server's WebRTC answer, built locally from invite data so no signalling server is
//! needed (ADR 0008).

/// What an invite carries for WebRTC: the server's address and ICE credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebRtcInvite {
    pub ip: String,
    pub port: u16,
    pub ufrag: String,
    pub pwd: String,
}

/// The server's answer SDP; `mid` must match the offer's data m-line.
pub fn build_server_answer(invite: &WebRtcInvite, cert_hash: &[u8], mid: &str) -> String {
    let fingerprint = cert_hash
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    let ip_version = if invite.ip.contains(':') {
        "IP6"
    } else {
        "IP4"
    };
    let ip = &invite.ip;
    [
        "v=0".to_string(),
        format!("o=- 1 1 IN {ip_version} {ip}"),
        "s=-".into(),
        "t=0 0".into(),
        format!("a=group:BUNDLE {mid}"),
        "a=ice-lite".into(),
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel".into(),
        format!("c=IN {ip_version} {ip}"),
        format!("a=mid:{mid}"),
        format!("a=ice-ufrag:{}", invite.ufrag),
        format!("a=ice-pwd:{}", invite.pwd),
        format!("a=fingerprint:sha-256 {fingerprint}"),
        "a=setup:passive".into(),
        "a=sctp-port:5000".into(),
        "a=max-message-size:262144".into(),
        format!(
            "a=candidate:1 1 udp 2130706431 {ip} {} typ host",
            invite.port
        ),
        "a=end-of-candidates".into(),
        String::new(),
    ]
    .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invite(ip: &str) -> WebRtcInvite {
        WebRtcInvite {
            ip: ip.into(),
            port: 4434,
            ufrag: "abcd".into(),
            pwd: "p".repeat(22),
        }
    }

    #[test]
    fn describes_the_ice_lite_server_with_its_fingerprint_and_host_candidate() {
        let hash: Vec<u8> = (0..32).collect();
        let sdp = build_server_answer(&invite("127.0.0.1"), &hash, "0");
        assert!(sdp.contains("a=ice-lite"));
        assert!(sdp.contains("a=setup:passive"));
        assert!(sdp.contains("a=fingerprint:sha-256 00:01:02:03:"));
        assert!(sdp.contains("a=candidate:1 1 udp 2130706431 127.0.0.1 4434 typ host"));
        assert!(sdp.ends_with("\r\n"));
    }

    #[test]
    fn uses_ip6_for_ipv6_servers() {
        let sdp = build_server_answer(&invite("::1"), &[0; 32], "data");
        assert!(sdp.contains("c=IN IP6 ::1"));
        assert!(sdp.contains("a=mid:data"));
    }
}
