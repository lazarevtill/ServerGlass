//! Two devices pairing for real: a listener, a scanner, a socket between them.
//!
//! The unit tests exercise the handshake arithmetic; this exercises the thing the user does —
//! one device shows a code, the other scans it, and an inventory arrives.

use std::time::Duration;

use sg_sync::payload::{Payload, SyncHost};
use sg_sync::transfer::{accept_transfer, send_transfer, write_payload, Listener};
use sg_sync::{merge, Offer};

fn inventory() -> Payload {
    Payload {
        hosts: vec![SyncHost {
            address: "10.0.0.9".into(),
            port: 2222,
            user: "root".into(),
            auth_kind: "password".into(),
            key_path: None,
            host_key_policy: "accept_new".into(),
            refresh_ms: 1000,
        }],
        known_hosts: vec!["[10.0.0.9]:2222 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5".into()],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_inventory_crosses_between_two_devices() {
    // The device being set up shows the QR.
    let listener = Listener::bind(&["127.0.0.1".to_string()])
        .await
        .expect("bind");
    let qr = listener.offer().encode();

    let receiving = tokio::spawn(async move {
        let (session, mut stream) = listener.accept().await.expect("accept");
        let code = session.verification_code();
        let bytes = accept_transfer(&session, &mut stream)
            .await
            .expect("receive");
        (code, Payload::from_json(&bytes).expect("payload"))
    });

    // The device that already has the servers scans it.
    let offer = Offer::decode(&qr).expect("decode the scanned code");
    let (session, mut stream) = tokio::time::timeout(Duration::from_secs(5), send_transfer(&offer))
        .await
        .expect("connect in time")
        .expect("connect");

    let sender_code = session.verification_code();

    // Only now — after a human would have compared the codes — is anything sent.
    write_payload(&session, &mut stream, &inventory().to_json().unwrap())
        .await
        .expect("send");

    let (receiver_code, received) = tokio::time::timeout(Duration::from_secs(5), receiving)
        .await
        .expect("receive in time")
        .expect("task");

    assert_eq!(
        sender_code, receiver_code,
        "the two devices showed different codes, so a user comparing them would refuse a valid pairing"
    );
    assert_eq!(received, inventory());

    // And a fresh device merging it starts from nothing and ends with the inventory.
    let merged = merge(&Payload::default(), &received);
    assert_eq!(merged.added_hosts, 1);
    assert_eq!(merged.added_pins, 1);
    assert!(merged.conflicts.is_empty());
}

/// A device that already knows a host with a *different* key must keep its own and say so.
#[tokio::test(flavor = "multi_thread")]
async fn a_transferred_pin_never_silently_replaces_a_trusted_one() {
    let listener = Listener::bind(&["127.0.0.1".to_string()])
        .await
        .expect("bind");
    let qr = listener.offer().encode();

    let receiving = tokio::spawn(async move {
        let (session, mut stream) = listener.accept().await.expect("accept");
        let bytes = accept_transfer(&session, &mut stream)
            .await
            .expect("receive");
        Payload::from_json(&bytes).expect("payload")
    });

    let offer = Offer::decode(&qr).expect("decode");
    let (session, mut stream) = send_transfer(&offer).await.expect("connect");

    // The sending device carries a different key for the same host.
    let mut impostor = inventory();
    impostor.known_hosts = vec!["[10.0.0.9]:2222 ssh-ed25519 SOMETHINGELSE".into()];
    write_payload(&session, &mut stream, &impostor.to_json().unwrap())
        .await
        .expect("send");

    let received = tokio::time::timeout(Duration::from_secs(5), receiving)
        .await
        .expect("in time")
        .expect("task");

    let merged = merge(&inventory(), &received);
    assert_eq!(
        merged.conflicts.len(),
        1,
        "the disagreement was not reported"
    );
    assert!(
        merged
            .known_hosts
            .iter()
            .all(|l| l.contains("AAAAC3NzaC1lZDI1NTE5")),
        "the trusted key was overwritten by the transfer: {:?}",
        merged.known_hosts
    );
}

/// Anything on the local network can connect to the listener. A peer that is not running this
/// protocol must be rejected rather than crash or hang the device being set up.
#[tokio::test(flavor = "multi_thread")]
async fn a_stranger_connecting_to_the_listener_is_rejected() {
    let listener = Listener::bind(&["127.0.0.1".to_string()])
        .await
        .expect("bind");
    let address = listener.offer().addresses[0].clone();

    let accepting = tokio::spawn(async move { listener.accept().await });

    // Connect and send junk where a public key should be.
    let mut stream = tokio::net::TcpStream::connect(&address)
        .await
        .expect("connect");
    tokio::io::AsyncWriteExt::write_all(&mut stream, b"hello?")
        .await
        .expect("write");
    drop(stream);

    let outcome = tokio::time::timeout(Duration::from_secs(5), accepting)
        .await
        .expect("the listener hung on a stranger")
        .expect("task");
    assert!(outcome.is_err(), "junk was accepted as a handshake");
}

/// The VPN case: a device advertises several addresses and only one of them reaches.
///
/// A phone with Wi-Fi and WireGuard up has at least two addresses, and which one the other device
/// can dial depends on where that device is. Advertising one and guessing wrong is the difference
/// between pairing working over a tunnel and not working at all — so the scanner tries each in turn
/// and the first that answers wins.
#[tokio::test(flavor = "multi_thread")]
async fn pairing_succeeds_when_only_a_later_address_is_reachable() {
    let listener = Listener::bind(&["127.0.0.1".to_string()])
        .await
        .expect("bind");
    let reachable = listener.offer().addresses[0].clone();

    // What the QR would carry on a device with a LAN address, a dead VPN address, and the one that
    // actually works — in the order the platform happened to enumerate them.
    let mut offer = listener.offer().clone();
    offer.addresses = vec![
        // Reserved for documentation; nothing answers, and it is not routed here.
        "192.0.2.1:9".to_string(),
        // A plausible tunnel address that is not up.
        "[100::1]:9".to_string(),
        reachable,
    ];

    let receiving = tokio::spawn(async move {
        let (session, mut stream) = listener.accept().await.expect("accept");
        let bytes = accept_transfer(&session, &mut stream)
            .await
            .expect("receive");
        (
            session.verification_code(),
            Payload::from_json(&bytes).expect("payload"),
        )
    });

    let (session, mut stream) =
        tokio::time::timeout(Duration::from_secs(20), send_transfer(&offer))
            .await
            .expect("the scanner hung instead of trying the next address")
            .expect("connect via the reachable address");

    write_payload(&session, &mut stream, &inventory().to_json().unwrap())
        .await
        .expect("send");

    let (code, received) = tokio::time::timeout(Duration::from_secs(10), receiving)
        .await
        .expect("in time")
        .expect("task");

    assert_eq!(session.verification_code(), code);
    assert_eq!(received, inventory());
}

/// If nothing answers, say something a person can act on rather than a socket error.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_offer_explains_itself() {
    let offer = Offer {
        public_key: [7u8; 32],
        nonce: [0u8; 16],
        addresses: vec!["192.0.2.1:9".into()],
    };

    let outcome = tokio::time::timeout(Duration::from_secs(20), send_transfer(&offer))
        .await
        .expect("gave up in reasonable time");

    let text = match outcome {
        Ok(_) => panic!("connected to an address where nothing is listening"),
        Err(e) => e.to_string(),
    };
    assert!(
        text.contains("same network or VPN"),
        "unhelpful failure: {text}"
    );
}
