//! Carrying the payload between two devices on the same network.
//!
//! Directly, over TCP, on the local network: no server, no account, nothing in anybody's cloud.
//! The case this exists for is two devices in the same room on the same Wi-Fi, which is also the
//! case where a QR is scannable — the two constraints coincide, so neither is a compromise.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::pairing::{Handshake, Offer, Session};
use crate::SyncError;

/// The largest transfer accepted, before decryption.
///
/// An inventory is kilobytes. This is a bound on what an unauthenticated peer can make the device
/// allocate — the listener is open on a local network and anything on it can connect.
const MAX_TRANSFER: usize = 1 << 20;

/// A device showing a QR and waiting for the other to connect.
pub struct Listener {
    listener: TcpListener,
    handshake: Handshake,
    offer: Offer,
}

impl Listener {
    /// Bind, and produce the offer to encode into a QR.
    ///
    /// `advertise_hosts` are every address this device might be reachable at — the caller supplies
    /// them because only the platform can enumerate its interfaces. Pass all of them: the Wi-Fi
    /// address, and the VPN address if a tunnel is up. Which one works depends on where the other
    /// device is, and a device on WireGuard or Tailscale is often reachable at *only* the tunnel
    /// address.
    ///
    /// The socket itself binds `0.0.0.0`, so it is listening on every interface regardless; the
    /// list only decides what the other device is told to dial.
    pub async fn bind(advertise_hosts: &[String]) -> Result<Self, SyncError> {
        if advertise_hosts.is_empty() {
            return Err(SyncError::Transfer(
                "this device has no network address to offer".into(),
            ));
        }
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| SyncError::Transfer(format!("could not listen: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| SyncError::Transfer(e.to_string()))?
            .port();

        let addresses = advertise_hosts
            .iter()
            .map(|host| {
                // An IPv6 literal has to be bracketed before a port is appended, or `host:port`
                // is unparseable. A VPN handing out v6 is exactly where this bites.
                if host.contains(':') && !host.starts_with('[') {
                    format!("[{host}]:{port}")
                } else {
                    format!("{host}:{port}")
                }
            })
            .collect();
        let (handshake, offer) = Handshake::offering(addresses);
        Ok(Listener {
            listener,
            handshake,
            offer,
        })
    }

    pub fn offer(&self) -> &Offer {
        &self.offer
    }

    /// Wait for the other device, complete the handshake, and hand back the session.
    ///
    /// Returns before anything is transferred: the caller must show the verification code and get
    /// the user's confirmation first. That ordering is the point of the code.
    pub async fn accept(self) -> Result<(Session, TcpStream), SyncError> {
        let (mut stream, _) = self
            .listener
            .accept()
            .await
            .map_err(|e| SyncError::Transfer(format!("no device connected: {e}")))?;

        let mut theirs = [0u8; 32];
        stream
            .read_exact(&mut theirs)
            .await
            .map_err(|e| SyncError::Transfer(format!("handshake failed: {e}")))?;

        stream
            .write_all(&self.handshake.public_key())
            .await
            .map_err(|e| SyncError::Transfer(format!("handshake failed: {e}")))?;

        let session = self.handshake.complete(theirs, true)?;
        Ok((session, stream))
    }
}

/// How long to wait on one candidate address before trying the next.
///
/// A wrong address on a local network usually refuses immediately, but a VPN address that is routed
/// yet unreachable will hang until the OS gives up — which would otherwise strand the user on a
/// spinner while a perfectly good second address goes untried.
const PER_ADDRESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Dial a scanned offer and complete the handshake.
///
/// Tries every advertised address in order and uses the first that answers, which is what makes
/// this work when the two devices are on a VPN rather than the same Wi-Fi — or on both, where only
/// one of the two routes actually reaches.
///
/// Like [`Listener::accept`], this returns before anything is sent — the verification code has to
/// reach a human first.
pub async fn send_transfer(offer: &Offer) -> Result<(Session, TcpStream), SyncError> {
    let mut last: Option<String> = None;
    let mut stream = None;

    for address in &offer.addresses {
        match tokio::time::timeout(PER_ADDRESS_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(connected)) => {
                stream = Some(connected);
                break;
            }
            Ok(Err(e)) => last = Some(format!("{address}: {e}")),
            Err(_) => last = Some(format!("{address}: timed out")),
        }
    }

    let mut stream = stream.ok_or_else(|| {
        SyncError::Transfer(format!(
            "could not reach the other device. Check both are on the same network or VPN{}",
            last.map(|e| format!(" ({e})")).unwrap_or_default()
        ))
    })?;

    let handshake = Handshake::accepting(offer);
    stream
        .write_all(&handshake.public_key())
        .await
        .map_err(|e| SyncError::Transfer(format!("handshake failed: {e}")))?;

    let mut theirs = [0u8; 32];
    stream
        .read_exact(&mut theirs)
        .await
        .map_err(|e| SyncError::Transfer(format!("handshake failed: {e}")))?;

    let session = handshake.complete(theirs, false)?;
    Ok((session, stream))
}

/// Send the sealed payload. Call only after the user has confirmed the codes match.
pub async fn write_payload(
    session: &Session,
    stream: &mut TcpStream,
    plaintext: &[u8],
) -> Result<(), SyncError> {
    let sealed = session.seal(plaintext)?;
    let len = u32::try_from(sealed.len())
        .map_err(|_| SyncError::Transfer("the inventory is too large to send".into()))?;

    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| SyncError::Transfer(e.to_string()))?;
    stream
        .write_all(&sealed)
        .await
        .map_err(|e| SyncError::Transfer(e.to_string()))?;
    stream
        .flush()
        .await
        .map_err(|e| SyncError::Transfer(e.to_string()))?;
    Ok(())
}

/// Read and open the sealed payload. Call only after the user has confirmed.
pub async fn accept_transfer(
    session: &Session,
    stream: &mut TcpStream,
) -> Result<Vec<u8>, SyncError> {
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|e| SyncError::Transfer(format!("the transfer stopped early: {e}")))?;
    let len = u32::from_be_bytes(len) as usize;

    if len > MAX_TRANSFER {
        return Err(SyncError::Transfer(
            "the other device offered more than an inventory could be".into(),
        ));
    }

    let mut sealed = vec![0u8; len];
    stream
        .read_exact(&mut sealed)
        .await
        .map_err(|e| SyncError::Transfer(format!("the transfer stopped early: {e}")))?;

    session.open(&sealed)
}
