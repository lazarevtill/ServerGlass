//! The pairing handshake.
//!
//! Two devices in the same room, one showing a QR and the other scanning it. The camera is an
//! out-of-band channel: it needs physical presence and carries far more entropy than anyone would
//! type. What it is *not* is private — a screen can be photographed, filmed, or read across a room,
//! and a screenshot of a QR is as good as the original.
//!
//! So the QR carries a **public** key. Anyone who captures it learns a public key and nothing else.
//!
//! What that alone does not stop is someone scanning the code before the intended device does, or a
//! machine on the same network racing to answer. Both sides therefore derive the same six-digit
//! code from the full transcript and show it; the user checks the two screens match. That is the
//! same defence as Bluetooth numeric comparison and Signal's safety numbers, and it is the step
//! that turns "encrypted to somebody" into "encrypted to the device in my other hand".

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::SyncError;

/// Wire format version. A device running an older build must say so rather than misparse.
pub const VERSION: u8 = 1;

/// How long an offer is worth showing before it should be regenerated.
///
/// Not enforced by the protocol — a stale QR simply fails to connect once the listener is gone —
/// but the UI should expire the code so a screen left unlocked on a desk is not an open door.
pub const OFFER_TTL_SECS: u64 = 120;

/// What the QR encodes.
///
/// Deliberately small: a public key, a session nonce and where to reach the device. Roughly a
/// hundred bytes against a QR's ~2,900-byte ceiling, which is the reason the payload itself is
/// never in the code — a QR dense enough to hold an inventory is a QR that will not scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub public_key: [u8; 32],
    /// Fresh per offer. Binds the verification code to this pairing and no other, so a code
    /// observed once cannot be replayed against a later session.
    pub nonce: [u8; 16],
    /// Every address the offering device might be reachable at, most-likely first.
    ///
    /// A list rather than one address because a device usually has several, and which one works
    /// depends on where the *other* device is. A phone on the same Wi-Fi wants the LAN address; a
    /// laptop reaching it over WireGuard or Tailscale wants the VPN address, and the two are on
    /// different interfaces. Advertising only one guesses wrong roughly whenever a VPN is up.
    ///
    /// The scanner tries them in order and uses the first that answers, so listing extras costs a
    /// failed connection attempt, and omitting the right one costs the whole pairing.
    pub addresses: Vec<String>,
}

impl Offer {
    /// `SG1:<base64 key>:<base64 nonce>:<addr>,<addr>,…`
    ///
    /// Text rather than binary because it goes through a QR encoder on four platforms, and every
    /// one of them handles ASCII without argument. Addresses are comma-separated because an IPv6
    /// address is full of colons — a VPN that hands out v6 would otherwise break the parse.
    pub fn encode(&self) -> String {
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "SG{}:{}:{}:{}",
            VERSION,
            b64.encode(self.public_key),
            b64.encode(self.nonce),
            self.addresses.join(",")
        )
    }

    pub fn decode(text: &str) -> Result<Self, SyncError> {
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let text = text.trim();

        let rest = text
            .strip_prefix("SG")
            .ok_or_else(|| SyncError::Malformed("not a ServerGlass pairing code".into()))?;
        let (version, rest) = rest
            .split_once(':')
            .ok_or_else(|| SyncError::Malformed("truncated pairing code".into()))?;
        // A newer device offering a format this build does not know must be told plainly, not
        // shown a parse error about base64.
        if version != VERSION.to_string() {
            return Err(SyncError::Version(version.to_string()));
        }

        let mut parts = rest.splitn(3, ':');
        let key = parts
            .next()
            .ok_or_else(|| SyncError::Malformed("no key".into()))?;
        let nonce = parts
            .next()
            .ok_or_else(|| SyncError::Malformed("no nonce".into()))?;
        let addresses = parts
            .next()
            .ok_or_else(|| SyncError::Malformed("no address".into()))?;

        let key: [u8; 32] = b64
            .decode(key)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| SyncError::Malformed("bad key".into()))?;
        let nonce: [u8; 16] = b64
            .decode(nonce)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| SyncError::Malformed("bad nonce".into()))?;

        let addresses: Vec<String> = addresses
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        if addresses.is_empty() {
            return Err(SyncError::Malformed("no address".into()));
        }

        Ok(Offer {
            public_key: key,
            nonce,
            addresses,
        })
    }
}

/// One side's ephemeral key material. Discarded when the pairing ends, successfully or not.
pub struct Handshake {
    secret: StaticSecret,
    public: PublicKey,
    nonce: [u8; 16],
}

impl Handshake {
    /// The device showing the QR.
    pub fn offering(addresses: Vec<String>) -> (Self, Offer) {
        let secret = StaticSecret::random_from_rng(&mut rand::rng());
        let public = PublicKey::from(&secret);
        let mut nonce = [0u8; 16];
        rand::fill(&mut nonce);

        let offer = Offer {
            public_key: public.to_bytes(),
            nonce,
            addresses,
        };
        (
            Handshake {
                secret,
                public,
                nonce,
            },
            offer,
        )
    }

    /// The device that scanned it.
    pub fn accepting(offer: &Offer) -> Self {
        let secret = StaticSecret::random_from_rng(&mut rand::rng());
        let public = PublicKey::from(&secret);
        Handshake {
            secret,
            public,
            nonce: offer.nonce,
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.public.to_bytes()
    }

    /// Complete the exchange.
    ///
    /// `offering` says which side we are, because the transcript must be in the same order on both
    /// — otherwise the two devices derive different keys and different codes, and the failure looks
    /// like a mismatched verification code rather than a bug.
    pub fn complete(self, their_public: [u8; 32], offering: bool) -> Result<Session, SyncError> {
        let theirs = PublicKey::from(their_public);
        let shared = self.secret.diffie_hellman(&theirs);

        // Reject the all-zero shared secret: it is what a peer sending a low-order point produces,
        // and continuing would mean encrypting to a key an attacker knows.
        if !shared.was_contributory() {
            return Err(SyncError::Handshake(
                "the other device sent an unusable key".into(),
            ));
        }

        let (first, second) = if offering {
            (self.public.to_bytes(), their_public)
        } else {
            (their_public, self.public.to_bytes())
        };

        let mut transcript = Vec::with_capacity(80);
        transcript.push(VERSION);
        transcript.extend_from_slice(&first);
        transcript.extend_from_slice(&second);
        transcript.extend_from_slice(&self.nonce);

        let hkdf = Hkdf::<Sha256>::new(Some(&transcript), shared.as_bytes());

        let mut key = [0u8; 32];
        hkdf.expand(b"serverglass pairing key", &mut key)
            .map_err(|_| SyncError::Handshake("key derivation failed".into()))?;

        let mut sas = [0u8; 4];
        hkdf.expand(b"serverglass verification code", &mut sas)
            .map_err(|_| SyncError::Handshake("key derivation failed".into()))?;

        Ok(Session {
            key,
            // Six digits: enough that guessing is a one-in-a-million shot at a code that is only
            // valid for this one live exchange, few enough that a person reads it across correctly.
            verification_code: u32::from_be_bytes(sas) % 1_000_000,
        })
    }
}

/// A completed handshake: a key to encrypt with, and a code for the humans to compare.
pub struct Session {
    key: [u8; 32],
    verification_code: u32,
}

impl Session {
    /// Six digits, zero-padded, grouped for reading aloud: `042 913`.
    pub fn verification_code(&self) -> String {
        let code = format!("{:06}", self.verification_code);
        format!("{} {}", &code[..3], &code[3..])
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
        let cipher = ChaCha20Poly1305::new(&Key::from(self.key));
        // A random nonce per message. The key is used for one transfer in one direction, so a
        // counter would do — but random removes the chance of a reuse bug if that ever changes.
        let mut nonce = [0u8; 12];
        rand::fill(&mut nonce);

        let mut sealed = cipher
            .encrypt(&Nonce::from(nonce), plaintext)
            .map_err(|_| SyncError::Crypto("could not encrypt the transfer".into()))?;

        let mut out = nonce.to_vec();
        out.append(&mut sealed);
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        if sealed.len() < 12 {
            return Err(SyncError::Crypto("the transfer was truncated".into()));
        }
        let (nonce, body) = sealed.split_at(12);
        let nonce: [u8; 12] = nonce.try_into().expect("split at 12");
        let cipher = ChaCha20Poly1305::new(&Key::from(self.key));
        cipher
            .decrypt(&Nonce::from(nonce), body)
            .map_err(|_| SyncError::Crypto("the transfer could not be decrypted".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Session, Session) {
        let (offerer, offer) = Handshake::offering(vec!["192.168.1.10:8765".into()]);
        let accepter = Handshake::accepting(&offer);
        let accepter_public = accepter.public_key();

        let a = offerer.complete(accepter_public, true).expect("offerer");
        let b = accepter
            .complete(offer.public_key, false)
            .expect("accepter");
        (a, b)
    }

    #[test]
    fn both_sides_derive_the_same_key_and_code() {
        let (a, b) = pair();
        assert_eq!(a.verification_code(), b.verification_code());

        let sealed = a.seal(b"the inventory").expect("seal");
        assert_eq!(b.open(&sealed).expect("open"), b"the inventory");
    }

    /// The code has to be the same on both screens *and* different between pairings, or comparing
    /// it proves nothing.
    #[test]
    fn a_different_pairing_gets_a_different_code() {
        let (first, _) = pair();
        let (second, _) = pair();
        assert_ne!(
            first.verification_code(),
            second.verification_code(),
            "two independent pairings produced the same code"
        );
    }

    /// Someone who scanned the QR and answered first gets a different code from the one the
    /// intended device shows — which is exactly what the user is comparing.
    #[test]
    fn an_impostor_answering_the_offer_produces_a_mismatched_code() {
        let (offerer, offer) = Handshake::offering(vec!["192.168.1.10:8765".into()]);

        let honest = Handshake::accepting(&offer);
        let impostor = Handshake::accepting(&offer);

        // The offerer completes against whoever answered — the impostor.
        let offerer_session = offerer
            .complete(impostor.public_key(), true)
            .expect("offerer");
        let honest_session = honest.complete(offer.public_key, false).expect("honest");

        assert_ne!(
            offerer_session.verification_code(),
            honest_session.verification_code(),
            "an impostor produced the same code as the honest device"
        );
    }

    /// A tampered ciphertext must fail rather than decrypt to something.
    #[test]
    fn a_modified_transfer_is_rejected() {
        let (a, b) = pair();
        let mut sealed = a.seal(b"the inventory").expect("seal");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(b.open(&sealed).is_err());
    }

    #[test]
    fn a_truncated_transfer_is_rejected() {
        let (a, b) = pair();
        let sealed = a.seal(b"the inventory").expect("seal");
        assert!(b.open(&sealed[..8]).is_err());
    }

    #[test]
    fn an_offer_survives_the_trip_through_a_qr() {
        let (_, offer) = Handshake::offering(vec!["192.168.1.10:8765".into()]);
        let text = offer.encode();
        assert!(text.starts_with("SG1:"));
        assert_eq!(Offer::decode(&text).expect("decode"), offer);
    }

    /// Scanning a QR from a bus stop, or a newer version of the app, must say so plainly.
    #[test]
    fn junk_and_future_versions_are_named_rather_than_mangled() {
        assert!(matches!(
            Offer::decode("https://example.com"),
            Err(SyncError::Malformed(_))
        ));
        assert!(matches!(
            Offer::decode("SG1:"),
            Err(SyncError::Malformed(_))
        ));
        assert!(matches!(
            Offer::decode("SG9:AAAA:BBBB:1.2.3.4:9"),
            Err(SyncError::Version(v)) if v == "9"
        ));
    }

    /// A peer that sends a low-order point forces a shared secret both sides "agree" on and an
    /// attacker knows. It must be refused rather than used.
    #[test]
    fn a_degenerate_public_key_is_refused() {
        let (offerer, _) = Handshake::offering(vec!["192.168.1.10:8765".into()]);
        assert!(matches!(
            offerer.complete([0u8; 32], true),
            Err(SyncError::Handshake(_))
        ));
    }
}
