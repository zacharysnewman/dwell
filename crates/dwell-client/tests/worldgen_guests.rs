//! A guest joining a world made from a worldgen config (ADR 0074 B7: a Chisel edit or a fixture,
//! `WorldMeta.worldgen`) gets the same terrain: the join's `WorldSettings` names the config's hash
//! in the required `WORLDGEN` tag, the guest asks for the text (`WorldgenRequest`), checks it and
//! builds its maps from it. A default world's join is unchanged. Session-level, natively, against
//! the local world's server (the friend-world host).

use dwell_client::local::LocalCore;
use dwell_client::net::session::{Out, Session, SessionState};
use dwell_protocol::Message;
use dwell_protocol::world_settings::tag;
use dwell_protocol::{Channel, TransportKind};
use dwell_sim::entropy::SeededEntropy;
use dwell_sim::server::{EditPolicy, OutKind};
use dwell_storage::StorageBackend;
use dwell_world::{Face, GENERATOR_TERRAIN};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::{worldgen_hash, worldgen_map_bytes};
use dwell_worldgen::worldmap::{cache, codec};
use ed25519_dalek::{Signer, SigningKey};

struct NoStorage;
impl StorageBackend for NoStorage {
    fn len(&self) -> Result<u64, std::io::Error> {
        unreachable!()
    }
    fn read(&self, _: u64, _: &mut [u8]) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn set_len(&self, _: u64) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn sync_data(&self) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<(), std::io::Error> {
        unreachable!()
    }
}

impl std::fmt::Debug for NoStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NoStorage")
    }
}

const GUEST: u32 = 2;
const FIXTURE: &str = "test:tall_peak";

/// Installs both faces' maps of world `seed` under `config`, as the shell does for a test world.
fn install(seed: u64, config: &str) {
    for face in [Face::A, Face::B] {
        let bytes = worldgen_map_bytes(face_seed(seed, face), config).expect("the config's maps");
        cache::install(&bytes).expect("installed");
    }
}

/// What the guest received, in order (decoded), and its session at the end.
struct Joined {
    session: Session,
    got: Vec<Message>,
    sent: Vec<Message>,
}

/// A guest joins the local world of `seed`; `tamper` may change each message the server sends it.
fn join(seed: u64, mut tamper: impl FnMut(Message) -> Message) -> Joined {
    let mut core = LocalCore::create(
        seed,
        GENERATOR_TERRAIN,
        None::<NoStorage>,
        Box::new(SeededEntropy(1)),
    )
    .unwrap();
    let host = SigningKey::from_bytes(&[5; 32]).verifying_key().to_bytes();
    core.host(4, EditPolicy::Everyone, EditPolicy::Everyone, host);
    core.connected(GUEST, TransportKind::WebRtc, [0; 32]);
    let key = SigningKey::from_bytes(&[6; 32]);
    let mut session = Session::new([0; 32], key.verifying_key().to_bytes(), "Guest", "t", 1e9);
    session.start(0.0);
    let (mut got, mut sent) = (Vec::new(), Vec::new());
    for step in 0..600 {
        let now = f64::from(step) * 1000.0 / 60.0;
        if matches!(session.state(), SessionState::Confirming { .. }) {
            session.accept_registry();
        }
        for out in session.take_out() {
            match out {
                Out::Reliable(b) => {
                    sent.extend(Message::decode(&b));
                    core.reliable(GUEST, Channel::Control, &b);
                }
                Out::Datagram(b) => core.datagram(GUEST, &b),
                Out::Sign(t) => session.signed(key.sign(&t).to_bytes()),
                Out::Close => {}
            }
        }
        core.advance(1.0 / 60.0);
        for o in core.take_outbox() {
            if o.session != GUEST {
                continue;
            }
            if let OutKind::Reliable(channel) = o.kind {
                let m = tamper(Message::decode(&o.bytes).expect("a message"));
                got.push(m.clone());
                session.on_reliable(channel, &m.encode(), now);
            }
        }
        if matches!(
            session.state(),
            SessionState::Joined { .. } | SessionState::Closed { .. }
        ) {
            break;
        }
    }
    Joined { session, got, sent }
}

fn settings_of(got: &[Message]) -> dwell_protocol::world_settings::SettingsFields {
    got.iter()
        .find_map(|m| match m {
            Message::WorldSettings(f) => Some(f.clone()),
            _ => None,
        })
        .expect("WorldSettings")
}

fn closed_message(s: &Session) -> String {
    match s.state() {
        SessionState::Closed { message } => message.clone(),
        other => panic!("not closed: {other:?}"),
    }
}

#[test]
fn a_default_worlds_join_is_unchanged() {
    let j = join(0xB7_0001, |m| m);
    assert!(matches!(j.session.state(), SessionState::Joined { .. }));
    assert_eq!(j.session.worldgen_config(), "");
    let f = settings_of(&j.got);
    assert_eq!(f.worldgen_hash(), None);
    // The bytes are the saved fields' as before: clock, water speed, mode, nothing more.
    assert_eq!(
        f.0.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![tag::CLOCK, tag::WATER_SPEED, tag::MODE]
    );
    assert!(!j.sent.contains(&Message::WorldgenRequest));
    assert!(
        !j.got
            .iter()
            .any(|m| matches!(m, Message::WorldgenConfig(_)))
    );
}

// both-faces: compares each face's map, as the guest builds it, with the host's.
#[test]
fn a_test_worlds_guest_gets_its_config_and_builds_the_hosts_maps() {
    let seed = 0xB7_0002;
    install(seed, FIXTURE);
    let j = join(seed, |m| m);
    assert!(
        matches!(j.session.state(), SessionState::Joined { .. }),
        "{:?}",
        j.session.state()
    );
    let f = settings_of(&j.got);
    assert_eq!(f.worldgen_hash(), Some(worldgen_hash(FIXTURE)));
    assert!(j.sent.contains(&Message::WorldgenRequest));
    let text = j.session.worldgen_config().to_owned();
    assert_eq!(text, FIXTURE);
    // The guest's maps, built from the text it was sent, are the host's, on both faces: the same
    // bytes and the same ground at sample columns.
    for face in [Face::A, Face::B] {
        let s = face_seed(seed, face);
        let host = cache::for_seed(s);
        assert_eq!(
            host.worldgen_text(),
            FIXTURE,
            "{face:?}: the host's is the config's"
        );
        let guest_bytes = worldgen_map_bytes(s, &text).unwrap();
        assert_eq!(guest_bytes, codec::encode(&host), "{face:?}");
        let guest = codec::decode(&guest_bytes).unwrap();
        for (x, z) in [
            (0.0, 0.0),
            (300.0, -200.0),
            (-1500.0, 900.0),
            (2400.0, 2400.0),
        ] {
            assert_eq!(
                guest.height_at(x, z),
                host.height_at(x, z),
                "{face:?} at {x},{z}"
            );
        }
    }
}

#[test]
fn a_config_that_does_not_match_its_hash_is_refused_with_a_message() {
    let seed = 0xB7_0003;
    install(seed, FIXTURE);
    let j = join(seed, |m| match m {
        Message::WorldgenConfig(_) => Message::WorldgenConfig("{\"world\":{}}".into()),
        m => m,
    });
    assert!(closed_message(&j.session).contains("damaged"));
}

#[test]
fn an_older_guest_refuses_a_test_world_with_the_standard_message() {
    let seed = 0xB7_0004;
    install(seed, FIXTURE);
    // An older build reads WORLDGEN as a required tag it doesn't know: the same as this one
    // reading an unknown required tag (0x1f).
    let j = join(seed, |m| match m {
        Message::WorldSettings(mut f) => {
            for (t, _) in &mut f.0 {
                if *t == tag::WORLDGEN {
                    *t = 0x1f;
                }
            }
            Message::WorldSettings(f)
        }
        m => m,
    });
    assert_eq!(
        closed_message(&j.session),
        "This world needs a newer version of Dwell."
    );
    assert!(!j.sent.contains(&Message::WorldgenRequest));
}
