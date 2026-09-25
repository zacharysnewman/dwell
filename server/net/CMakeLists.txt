# Network front-end (native only). The Rust crate in ./wt provides WebTransport and, later, WebRTC
# behind a C ABI (ADRs 0001, 0008); Corrosion builds it with cargo and links it statically.
corrosion_import_crate(MANIFEST_PATH wt/Cargo.toml CRATES dwell-net)
target_include_directories(dwell_net INTERFACE wt/include)
