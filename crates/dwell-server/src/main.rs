//! Dwell dedicated server (ARCHITECTURE.md §4, §10.1): the server core behind the network
//! front-end. Transport events are drained into the core, the core steps at SIM_HZ, and its outbox
//! is sent back out.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dwell_net::{Command, Event, Limits, Master, NetServer, StartOptions};
use dwell_protocol::{
    Channel, MAX_DATAGRAM_BYTES, MAX_RELIABLE_MESSAGE_BYTES, PROTOCOL_VERSION, SIM_HZ,
    TransportKind,
};
use dwell_server::*;
use dwell_sim::app_version::APP_VERSION;
use dwell_sim::entropy::SystemEntropy;
use dwell_sim::fixed_step::FixedStep;
use dwell_sim::server::{OutKind, Server};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Ok(mut options) = parse_options(&args) else {
        println!("{USAGE}");
        std::process::exit(2);
    };
    match open_world(&mut options) {
        Ok(lines) => lines.iter().for_each(|l| println!("{l}")),
        Err(e) => {
            eprintln!("dwell-server {APP_VERSION}: {e}");
            std::process::exit(1);
        }
    }
    // Terrain generation off the tick (§6.3): about cores − 2 threads.
    let cores = std::thread::available_parallelism().map_or(2, usize::from);
    options.server.worldgen_threads = cores.saturating_sub(2).max(1);
    let advertised: IpAddr = match options.advertise.parse() {
        Ok(ip) => ip,
        Err(_) => {
            eprintln!(
                "dwell-server: --advertise {:?} is not an IP address",
                options.advertise
            );
            std::process::exit(2);
        }
    };
    let net = match NetServer::start(StartOptions {
        port: options.port,
        rtc_port: options.rtc_port,
        advertised_ip: advertised,
        limits: Limits {
            max_reliable_message_bytes: MAX_RELIABLE_MESSAGE_BYTES as u32,
            max_datagram_bytes: MAX_DATAGRAM_BYTES as u32,
        },
    }) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("dwell-server: failed to start networking: {e}");
            std::process::exit(1);
        }
    };
    let (ufrag, pwd) = net.ice_credentials();
    let listening = Listening {
        port: net.port(),
        rtc_port: net.rtc_port(),
        cert_hex: hex(&net.cert_hash()),
        ice_ufrag: ufrag.to_string(),
        ice_pwd: pwd.to_string(),
    };
    let master_url = options.master.clone();
    let server_key = options.server_key;
    let world_config = std::mem::take(&mut options.server);
    let mut server = Server::new(world_config, Box::new(SystemEntropy));

    println!("dwell-server {APP_VERSION} | protocol v{PROTOCOL_VERSION}");
    println!(
        "listening on UDP {} (WebTransport) and UDP {} (WebRTC)",
        listening.port, listening.rtc_port
    );
    println!("certificate sha-256: {}", listening.cert_hex);
    println!(
        "invite link: {}",
        invite_link(&options.client_url, &options.advertise, &listening)
    );

    // Registration with the master (§10.1): a heartbeat now and every heartbeat_s.
    let lan = lan_addresses();
    let master = if options.visibility == "none" {
        println!("master: not registering (--visibility none)");
        None
    } else {
        match Master::start(&master_url, server_key) {
            Ok(m) => {
                println!(
                    "master: registering with {master_url} as {}",
                    options.visibility
                );
                Some(m)
            }
            Err(e) => {
                eprintln!("dwell-server: master registration unavailable: {e}");
                None
            }
        }
    };
    // The heartbeat reports the live name and limits: the server's config, which hosting can change.
    options.server = Default::default();
    let heartbeat = |server: &Server, options: &mut Options| {
        if let Some(m) = &master {
            let c = server.config();
            options.server.name = c.name.clone();
            options.server.motd = c.motd.clone();
            options.server.max_players = c.max_players;
            m.heartbeat(heartbeat_body(
                options,
                &listening,
                &lan,
                server.joined_players(),
            ));
        }
    };
    heartbeat(&server, &mut options);
    let mut next_heartbeat = Instant::now() + Duration::from_secs(options.heartbeat_s);
    let (mut shown_code, mut shown_error) = (String::new(), String::new());

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        let _ = ctrlc::set_handler(move || running.store(false, Ordering::SeqCst));
    }

    let flush = |server: &mut Server| {
        for out in server.take_outbox() {
            let command = match out.kind {
                OutKind::Reliable(channel) => Command::Reliable {
                    channel: channel as u8,
                    data: out.bytes,
                },
                OutKind::Datagram => Command::Datagram(out.bytes),
                OutKind::Close => Command::Close,
            };
            net.send(out.session, command);
        }
    };

    let mut fixed = FixedStep::new(SIM_HZ);
    let mut last = Instant::now();
    while running.load(Ordering::SeqCst) {
        while let Some(event) = net.poll() {
            match event {
                Event::Connected {
                    session,
                    transport,
                    binding,
                } => {
                    let kind = if transport == 2 {
                        TransportKind::WebRtc
                    } else {
                        TransportKind::WebTransport
                    };
                    server.on_connected(session, kind, binding);
                }
                Event::Disconnected { session } => server.on_disconnected(session),
                Event::Reliable {
                    session,
                    channel,
                    data,
                } => {
                    if let Some(channel) = Channel::from_u8(channel) {
                        server.on_reliable(session, channel, &data);
                    }
                }
                Event::Datagram { session, data } => server.on_datagram(session, &data),
            }
        }
        // Replies to requests go out immediately rather than waiting for the next tick.
        flush(&mut server);

        let now = Instant::now();
        for _ in 0..fixed.advance((now - last).as_secs_f64()) {
            server.step();
        }
        last = now;
        flush(&mut server);

        if let Some(m) = &master {
            if now >= next_heartbeat {
                heartbeat(&server, &mut options);
                next_heartbeat = now + Duration::from_secs(options.heartbeat_s);
            }
            // Say once when the join code is known, and whenever registration starts or stops failing.
            let status = m.status();
            if !status.display.is_empty() && status.display != shown_code {
                shown_code = status.display.clone();
                println!(
                    "join code: {} (link: {}?code={})",
                    shown_code,
                    options.client_url,
                    shown_code.replace('-', "")
                );
            }
            if status.error != shown_error {
                shown_error = status.error.clone();
                if !shown_error.is_empty() {
                    eprintln!("master: {shown_error}");
                }
            }
        }
        // Wake for the next step, and at least every 2 ms to keep request latency low.
        std::thread::sleep(Duration::from_secs_f64(
            fixed.time_until_next_step().min(0.002),
        ));
    }

    println!("dwell-server: shutting down");
    // Tell joined players why (Reject(ServerClosing)); best effort before the transport stops.
    server.close_sessions("The server is shutting down.", false);
    flush(&mut server);
    // Leave the master's listings at once rather than after two missed heartbeats.
    if let Some(m) = master {
        m.stop(true);
    }
    net.shutdown();
    if server.config().store.is_some() {
        server.save_now();
        server.flush_store();
        let stats = server.save_stats();
        if stats.failed > 0 {
            eprintln!("dwell-server: saving failed: {}", stats.last_error);
            std::process::exit(1);
        }
        println!("world saved");
    }
}
