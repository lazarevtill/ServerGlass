//! The C ABI, for the front-end UniFFI cannot serve.
//!
//! UniFFI has Swift and Kotlin backends, and no C# one. `docs/WINDOWS.md` says to evaluate
//! `uniffi-bindgen-cs` and expect to need the documented fallback; evaluated, it is a version
//! behind — its latest release targets uniffi 0.31 while this workspace is on 0.32 — and with no
//! `.udl` in the tree there is no non-library mode to fall back on. So this is that fallback:
//! hand-written `extern "C"`, with `csbindgen` generating the C# declarations from these
//! signatures so neither side is a hand-transcribed copy of the other.
//!
//! # Why the payload is JSON and not C structs
//!
//! The obvious shape for this module is one `#[repr(C)]` struct per record. It is also the
//! dangerous one. [`TargetSnapshot`](crate::TargetSnapshot) is not flat: it holds
//! `Vec<MetricGauge>`, `Vec<DetailGroup>` which holds another `Vec<MetricGauge>`,
//! `Vec<EntityView>` likewise, an `Option<f64>` inside every gauge, and
//! [`ConnectionState`](crate::ConnectionState), which is a fielded enum. Describing all of that to
//! a C compiler means writing the same memory layout twice in two languages and keeping the copies
//! in step by hand — and "the same code shape on two platforms is not the same behaviour" is
//! already on this project's list of mistakes it has made.
//!
//! So the boundary is deliberately narrow: every function takes and returns a NUL-terminated UTF-8
//! string, an opaque handle, or a primitive. Nothing nested is ever described twice. The cost is a
//! serialise per refresh, which a desktop can afford — the snapshot is built and cloned per poll
//! today regardless — and the benefit is that drift becomes *testable*, which it cannot be when the
//! two layouts only agree by inspection. See `field_set_is_asserted_so_a_new_field_fails_here`
//! below, which is the same guard `crates/sg-sync` puts on the pairing wire format.
//!
//! # The envelope
//!
//! Every fallible call returns exactly one of:
//!
//! ```text
//! {"ok": <value>}
//! {"err": {"kind": "connection", "detail": "…", "recoverable": true}}
//! ```
//!
//! One code path, no out-parameters and no error codes — and `recoverable` survives the crossing,
//! which is what lets the UI say "ServerGlass will keep retrying" rather than guessing.
//!
//! # Memory
//!
//! Every `*mut c_char` returned here was allocated by Rust and **must** be handed back to
//! [`sg_string_free`]. The handle from [`sg_core_new`] must be handed back to [`sg_core_free`].

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::sync::{SyncBundle, SyncReceiver, SyncSender};
use crate::{ServerGlass, SgError, TargetConfig};

/// What the caller holds: opaque on the C side, this on ours.
pub struct CoreHandle {
    core: Arc<ServerGlass>,
    pairing: Mutex<Pairing>,
}

/// Live pairing sessions, addressed by number.
///
/// A `u64` into a registry rather than an `Arc::into_raw` pointer handed to C#. Pairing objects are
/// reference-counted and the C# side would have to get every free exactly right to avoid a double
/// free — which is not a class of bug worth inviting to save a hash lookup that happens twice per
/// pairing.
#[derive(Default)]
struct Pairing {
    next_id: u64,
    receivers: HashMap<u64, Arc<SyncReceiver>>,
    senders: HashMap<u64, Arc<SyncSender>>,
}

/// The error half of the envelope.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrBody {
    /// `unknownTarget`, `connection`, `pairing` or `internal`.
    kind: &'static str,
    detail: String,
    /// Whether retrying could ever help. Bad credentials and a changed host key cannot.
    recoverable: bool,
}

impl ErrBody {
    fn internal(detail: impl Into<String>) -> Self {
        ErrBody {
            kind: "internal",
            detail: detail.into(),
            recoverable: false,
        }
    }

    fn pairing(detail: impl Into<String>) -> Self {
        ErrBody {
            kind: "pairing",
            detail: detail.into(),
            recoverable: false,
        }
    }
}

impl From<SgError> for ErrBody {
    fn from(error: SgError) -> Self {
        match error {
            SgError::UnknownTarget { id } => ErrBody {
                kind: "unknownTarget",
                detail: format!("no such target: {id}"),
                recoverable: false,
            },
            SgError::Connection {
                detail,
                recoverable,
            } => ErrBody {
                kind: "connection",
                detail,
                recoverable,
            },
            SgError::Internal { detail } => ErrBody::internal(detail),
        }
    }
}

impl From<crate::sync::SyncError> for ErrBody {
    fn from(error: crate::sync::SyncError) -> Self {
        ErrBody::pairing(error.to_string())
    }
}

#[derive(Serialize)]
enum Envelope<T> {
    #[serde(rename = "ok")]
    Ok(T),
    #[serde(rename = "err")]
    Err(ErrBody),
}

/// Hand a JSON string to C, transferring ownership.
fn into_c(json: String) -> *mut c_char {
    // serde escapes any interior NUL rather than emitting one, so this cannot fail in practice —
    // but it is fallible, and a null pointer the caller has to special-case is a worse answer than
    // an envelope describing what went wrong.
    CString::new(json)
        .unwrap_or_else(|_| {
            CString::new(r#"{"err":{"kind":"internal","detail":"the reply contained a NUL","recoverable":false}}"#)
                .expect("the literal has no NUL")
        })
        .into_raw()
}

fn reply<T: Serialize>(result: Result<T, ErrBody>) -> *mut c_char {
    let envelope = match result {
        Ok(value) => Envelope::Ok(value),
        Err(body) => Envelope::Err(body),
    };
    match serde_json::to_string(&envelope) {
        Ok(json) => into_c(json),
        // Serialising the *reply* failed. Report that rather than the value, and build the message
        // by serialising a plain string so the detail cannot itself break the JSON.
        Err(error) => {
            let detail = serde_json::to_string(&format!("could not serialise the reply: {error}"))
                .unwrap_or_else(|_| "\"could not serialise the reply\"".to_string());
            into_c(format!(
                r#"{{"err":{{"kind":"internal","detail":{detail},"recoverable":false}}}}"#
            ))
        }
    }
}

/// Run a call, turning a panic into an error envelope.
///
/// A panic unwinding across the FFI boundary is undefined behaviour. Catching it here also means a
/// bug shows up as a message in the app rather than as the process disappearing.
fn guarded<T, F>(call: F) -> *mut c_char
where
    T: Serialize,
    F: FnOnce() -> Result<T, ErrBody>,
{
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(result) => reply(result),
        Err(_) => reply::<T>(Err(ErrBody::internal(
            "the core panicked; the operation was abandoned",
        ))),
    }
}

/// # Safety
///
/// `text` must be null, or a NUL-terminated string that stays valid for the duration of the call.
unsafe fn borrow(text: *const c_char, what: &str) -> Result<String, ErrBody> {
    if text.is_null() {
        return Err(ErrBody::internal(format!("{what} was null")));
    }
    unsafe { CStr::from_ptr(text) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| ErrBody::internal(format!("{what} was not valid UTF-8")))
}

/// # Safety
///
/// `core` must be null, or a handle from [`sg_core_new`] that has not been freed.
unsafe fn borrow_core<'a>(core: *mut c_void) -> Result<&'a CoreHandle, ErrBody> {
    unsafe { (core as *const CoreHandle).as_ref() }
        .ok_or_else(|| ErrBody::internal("the core handle was null"))
}

fn from_json<T: serde::de::DeserializeOwned>(json: &str, what: &str) -> Result<T, ErrBody> {
    serde_json::from_str(json)
        .map_err(|error| ErrBody::internal(format!("could not read {what}: {error}")))
}

// ---------------------------------------------------------------------------------------------
// Lifetime
// ---------------------------------------------------------------------------------------------

/// Build the core. Returns null only if it could not be created at all.
#[no_mangle]
pub extern "C" fn sg_core_new() -> *mut c_void {
    match catch_unwind(|| {
        Box::into_raw(Box::new(CoreHandle {
            core: ServerGlass::new(),
            pairing: Mutex::new(Pairing::default()),
        })) as *mut c_void
    }) {
        Ok(handle) => handle,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Release the core and stop every target it owns.
///
/// # Safety
///
/// `core` must be a handle from [`sg_core_new`] that has not already been freed, or null.
#[no_mangle]
pub unsafe extern "C" fn sg_core_free(core: *mut c_void) {
    if core.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(core as *mut CoreHandle) });
}

/// Release a string returned by any function in this module.
///
/// # Safety
///
/// `text` must be a pointer returned by this module and not yet freed, or null.
#[no_mangle]
pub unsafe extern "C" fn sg_string_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(text) });
}

// ---------------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------------

/// Register a host from a JSON `TargetConfig`. Does not connect.
///
/// # Safety
///
/// `core` must be a live handle and `config_json` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_add_target(
    core: *mut c_void,
    config_json: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let json = unsafe { borrow(config_json, "the config") }?;
        let config: TargetConfig = from_json(&json, "the config")?;
        Ok(handle.core.add_target(config))
    })
}

/// Connect and begin refreshing. Returns immediately.
///
/// # Safety
///
/// `core` must be a live handle and `target_id` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_start(core: *mut c_void, target_id: *const c_char) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let id = unsafe { borrow(target_id, "the target id") }?;
        handle.core.start(id).map_err(ErrBody::from)
    })
}

/// Stop refreshing and drop the connection.
///
/// # Safety
///
/// `core` must be a live handle and `target_id` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_stop(core: *mut c_void, target_id: *const c_char) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let id = unsafe { borrow(target_id, "the target id") }?;
        handle.core.stop(id).map_err(ErrBody::from)
    })
}

/// Stop a host and forget it.
///
/// # Safety
///
/// `core` must be a live handle and `target_id` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_remove_target(
    core: *mut c_void,
    target_id: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let id = unsafe { borrow(target_id, "the target id") }?;
        handle.core.remove_target(id).map_err(ErrBody::from)
    })
}

/// The most recent completed refresh, as a JSON `TargetSnapshot`.
///
/// Cheap enough to call on a display timer.
///
/// # Safety
///
/// `core` must be a live handle and `target_id` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_snapshot(core: *mut c_void, target_id: *const c_char) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let id = unsafe { borrow(target_id, "the target id") }?;
        handle.core.snapshot(id).map_err(ErrBody::from)
    })
}

/// Every registered target id, sorted.
///
/// # Safety
///
/// `core` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn sg_target_ids(core: *mut c_void) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        Ok(handle.core.target_ids())
    })
}

/// Run one command on the host and wait for what it printed.
///
/// **Blocks.** Call it off the UI thread — the core's own documentation says so, and there is no
/// PTY, so anything interactive will produce nothing useful until the sixty-second timeout.
///
/// # Safety
///
/// `core` must be a live handle; `target_id` and `command` NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn sg_run_command(
    core: *mut c_void,
    target_id: *const c_char,
    command: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let id = unsafe { borrow(target_id, "the target id") }?;
        let command = unsafe { borrow(command, "the command") }?;
        handle.core.run_command(id, command).map_err(ErrBody::from)
    })
}

// ---------------------------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------------------------

/// Format a value the way every ServerGlass UI formats it.
///
/// Exported so the Windows app cannot re-implement it. Number formatting drifting between
/// front-ends is exactly how the same host came to read differently on a phone and a desk.
///
/// # Safety
///
/// `core` must be a live handle and `unit_suffix` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_format(
    core: *mut c_void,
    value: f64,
    unit_suffix: *const c_char,
    binary_scaled: bool,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let unit = unsafe { borrow(unit_suffix, "the unit suffix") }?;
        Ok(handle.core.format(value, unit, binary_scaled))
    })
}

/// Format a duration the way every ServerGlass UI formats it.
///
/// # Safety
///
/// `core` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn sg_format_duration(core: *mut c_void, seconds: f64) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        Ok(handle.core.format_duration(seconds))
    })
}

/// Normalise a series to 0-1 for a sparkline, oldest first.
///
/// `history_json` is a JSON array of numbers; the reply is a JSON array of the same length.
///
/// Exported for the same reason [`sg_format`] is. The noise floor is a rule about *what the chart
/// claims* — that a disk creeping from 5.19% to 5.20% must not draw a cliff — and it was already
/// written by hand in Swift and again in Kotlin before it moved into the core. Writing it a third
/// time in C# is the version of that mistake this project has already documented.
///
/// # Safety
///
/// `core` must be a live handle and `history_json` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_sparkline_points(
    core: *mut c_void,
    history_json: *const c_char,
) -> *mut c_char {
    guarded(|| {
        // The handle is taken for consistency with every other call and to reject a freed core,
        // even though the normalisation itself is a pure function.
        let _ = unsafe { borrow_core(core) }?;
        let json = unsafe { borrow(history_json, "the history") }?;
        let history: Vec<f64> = from_json(&json, "the history")?;
        Ok(crate::sparkline_points(&history))
    })
}

// ---------------------------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------------------------

/// A listening pairing session, and the text to render as a QR.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiverStarted {
    id: u64,
    pairing_code: String,
}

/// A connected pairing session, and the code the user compares with the other screen.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SenderConnected {
    id: u64,
    verification_code: String,
}

fn take_receiver(handle: &CoreHandle, id: u64) -> Result<Arc<SyncReceiver>, ErrBody> {
    // Cloned out from under the lock rather than used in place: `await_connection` blocks until
    // the other device shows up, and holding the registry lock for that would freeze every other
    // pairing call in the app.
    handle
        .pairing
        .lock()
        .map_err(|_| ErrBody::pairing("the pairing registry was poisoned"))?
        .receivers
        .get(&id)
        .cloned()
        .ok_or_else(|| ErrBody::pairing("that pairing is no longer active"))
}

fn take_sender(handle: &CoreHandle, id: u64) -> Result<Arc<SyncSender>, ErrBody> {
    handle
        .pairing
        .lock()
        .map_err(|_| ErrBody::pairing("the pairing registry was poisoned"))?
        .senders
        .get(&id)
        .cloned()
        .ok_or_else(|| ErrBody::pairing("that pairing is no longer active"))
}

/// Offer this device as the destination for a transfer.
///
/// `advertise_hosts_json` is a JSON array of every address this device might be reachable at. Pass
/// all of them — over a tunnel the tunnel address is often the only one that works, and only the
/// platform can enumerate its interfaces.
///
/// # Safety
///
/// `core` must be a live handle and `advertise_hosts_json` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_start_receiving(
    core: *mut c_void,
    advertise_hosts_json: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let json = unsafe { borrow(advertise_hosts_json, "the address list") }?;
        let hosts: Vec<String> = from_json(&json, "the address list")?;

        let receiver = handle.core.start_receiving(hosts)?;
        let pairing_code = receiver.pairing_code();

        let mut registry = handle
            .pairing
            .lock()
            .map_err(|_| ErrBody::pairing("the pairing registry was poisoned"))?;
        registry.next_id += 1;
        let id = registry.next_id;
        registry.receivers.insert(id, receiver);

        Ok(ReceiverStarted { id, pairing_code })
    })
}

/// Block until the other device connects, then return the code to show.
///
/// **Nothing has been received at this point.** The caller shows this code, the user compares it
/// with the other screen, and only then calls [`sg_receiver_receive`]. The whole security of the
/// exchange rests on that comparison happening first.
///
/// # Safety
///
/// `core` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn sg_receiver_await_connection(core: *mut c_void, id: u64) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let receiver = take_receiver(handle, id)?;
        receiver.await_connection().map_err(ErrBody::from)
    })
}

/// Take the transfer. Call only after the user confirmed the codes match.
///
/// # Safety
///
/// `core` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn sg_receiver_receive(core: *mut c_void, id: u64) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let receiver = take_receiver(handle, id)?;
        receiver.receive().map_err(ErrBody::from)
    })
}

/// Connect to a pairing code from the other device, and return the code to compare.
///
/// Nothing has been sent when this returns.
///
/// # Safety
///
/// `core` must be a live handle and `code` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_scan_pairing_code(
    core: *mut c_void,
    code: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let code = unsafe { borrow(code, "the pairing code") }?;

        let sender = handle.core.scan_pairing_code(code)?;
        let verification_code = sender.verification_code();

        let mut registry = handle
            .pairing
            .lock()
            .map_err(|_| ErrBody::pairing("the pairing registry was poisoned"))?;
        registry.next_id += 1;
        let id = registry.next_id;
        registry.senders.insert(id, sender);

        Ok(SenderConnected {
            id,
            verification_code,
        })
    })
}

/// Send the bundle. Call only after the user confirmed the codes match.
///
/// # Safety
///
/// `core` must be a live handle and `bundle_json` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn sg_sender_send(
    core: *mut c_void,
    id: u64,
    bundle_json: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let json = unsafe { borrow(bundle_json, "the bundle") }?;
        let bundle: SyncBundle = from_json(&json, "the bundle")?;
        let sender = take_sender(handle, id)?;
        sender.send(bundle).map_err(ErrBody::from)
    })
}

/// Forget a pairing session, whichever side it was.
///
/// # Safety
///
/// `core` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn sg_pairing_forget(core: *mut c_void, id: u64) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Ok(handle) = (unsafe { borrow_core(core) }) else {
            return;
        };
        if let Ok(mut registry) = handle.pairing.lock() {
            registry.receivers.remove(&id);
            registry.senders.remove(&id);
        }
    }));
}

/// Apply a received bundle to what this device already has.
///
/// Pure: it decides, it does not store. The caller writes the result to its own keystore and shows
/// the conflicts — which are never applied.
///
/// # Safety
///
/// `core` must be a live handle; both arguments NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn sg_merge_bundle(
    core: *mut c_void,
    existing_json: *const c_char,
    incoming_json: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let handle = unsafe { borrow_core(core) }?;
        let existing: SyncBundle = from_json(
            &unsafe { borrow(existing_json, "the existing bundle") }?,
            "the existing bundle",
        )?;
        let incoming: SyncBundle = from_json(
            &unsafe { borrow(incoming_json, "the incoming bundle") }?,
            "the incoming bundle",
        )?;
        Ok(handle.core.merge_bundle(existing, incoming))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConnectionState;
    use std::collections::BTreeSet;

    /// Call a C function and read the reply back as JSON, freeing the string the way C# must.
    fn read(pointer: *mut c_char) -> serde_json::Value {
        assert!(!pointer.is_null(), "the reply pointer was null");
        let json = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .expect("the reply was UTF-8")
            .to_string();
        unsafe { sg_string_free(pointer) };
        serde_json::from_str(&json).expect("the reply was JSON")
    }

    fn c(text: &str) -> CString {
        CString::new(text).expect("no interior NUL")
    }

    fn keys(value: &serde_json::Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("expected an object")
            .keys()
            .cloned()
            .collect()
    }

    fn expected(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    const CONFIG: &str = r#"{
        "host": "example.test", "port": 22, "user": "root", "authKind": "agent",
        "keyPath": null, "keyText": null, "secret": null,
        "hostKeyPolicy": "strict", "knownHostsPath": null, "refreshMs": 1000
    }"#;

    #[test]
    fn a_target_can_be_added_started_and_polled_through_the_c_abi() {
        let core = sg_core_new();
        assert!(!core.is_null());

        let added = read(unsafe { sg_add_target(core, c(CONFIG).as_ptr()) });
        let id = added["ok"].as_str().expect("an id came back").to_string();

        let ids = read(unsafe { sg_target_ids(core) });
        assert_eq!(ids["ok"], serde_json::json!([id.clone()]));

        // A target that has never connected still renders, rather than the UI special-casing null.
        let snapshot = read(unsafe { sg_snapshot(core, c(&id).as_ptr()) });
        assert_eq!(snapshot["ok"]["state"]["kind"], "idle");
        assert_eq!(snapshot["ok"]["displayName"], "example.test");

        let removed = read(unsafe { sg_remove_target(core, c(&id).as_ptr()) });
        assert!(removed["ok"].is_null(), "a void call replies with ok:null");

        unsafe { sg_core_free(core) };
    }

    #[test]
    fn errors_cross_as_an_envelope_rather_than_a_crash() {
        let core = sg_core_new();

        let missing = read(unsafe { sg_snapshot(core, c("nope").as_ptr()) });
        assert_eq!(missing["err"]["kind"], "unknownTarget");
        assert_eq!(missing["err"]["recoverable"], false);

        // A null string is a caller mistake, not grounds for dereferencing it.
        let null = read(unsafe { sg_snapshot(core, std::ptr::null()) });
        assert_eq!(null["err"]["kind"], "internal");

        // So is a null handle.
        let no_core = read(unsafe { sg_target_ids(std::ptr::null_mut()) });
        assert_eq!(no_core["err"]["kind"], "internal");

        // Malformed input is reported, not guessed at.
        let bad = read(unsafe { sg_add_target(core, c("{not json").as_ptr()) });
        assert_eq!(bad["err"]["kind"], "internal");

        unsafe { sg_core_free(core) };
    }

    /// The shape C# actually emits, rather than the shape this test file finds convenient.
    ///
    /// `System.Text.Json` writes nulls by default, but one `DefaultIgnoreCondition` setting on the
    /// serialiser options — a line nobody thinks of as load-bearing — makes it omit them instead.
    /// Without `#[serde(default)]` on the optional fields that turns every single connection into
    /// "could not read the config: missing field `keyPath`". Both forms are pinned here.
    #[test]
    fn a_config_with_its_optional_fields_omitted_is_still_accepted() {
        let core = sg_core_new();

        let minimal = r#"{"host":"a.test","port":22,"user":"root","authKind":"agent",
                          "hostKeyPolicy":"strict","refreshMs":1000}"#;
        let added = read(unsafe { sg_add_target(core, c(minimal).as_ptr()) });
        assert!(
            added["ok"].is_string(),
            "a config with nulls omitted was rejected: {added}"
        );

        // A genuinely required field stays required — a missing port must fail loudly rather than
        // silently connecting to 0.
        let no_port = r#"{"host":"a.test","user":"root","authKind":"agent",
                          "hostKeyPolicy":"strict","refreshMs":1000}"#;
        let rejected = read(unsafe { sg_add_target(core, c(no_port).as_ptr()) });
        assert_eq!(rejected["err"]["kind"], "internal");

        unsafe { sg_core_free(core) };
    }

    #[test]
    fn formatting_crosses_so_the_windows_app_never_reimplements_it() {
        let core = sg_core_new();
        let kib = read(unsafe { sg_format(core, 1536.0, c("B").as_ptr(), true) });
        assert_eq!(kib["ok"], "1.5 KiB");
        let uptime = read(unsafe { sg_format_duration(core, 90_000.0) });
        assert_eq!(uptime["ok"], "1d 1h");
        unsafe { sg_core_free(core) };
    }

    /// The sparkline rule crosses too, so C# never grows its own copy of the noise floor.
    #[test]
    fn the_sparkline_noise_floor_crosses_rather_than_being_rewritten() {
        let core = sg_core_new();

        let moved = read(unsafe { sg_sparkline_points(core, c("[0,50,100]").as_ptr()) });
        assert_eq!(moved["ok"], serde_json::json!([0.0, 0.5, 1.0]));

        // Storage at 5.19% ticking to 5.20%: real movement of 0.01 on a magnitude of 5.2, which
        // range-scaling alone would draw as a full-height cliff.
        let flat = read(unsafe { sg_sparkline_points(core, c("[5.19,5.19,5.20]").as_ptr()) });
        let points: Vec<f64> = flat["ok"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|v| v.as_f64().expect("a number"))
            .collect();
        let span = points.iter().copied().fold(f64::MIN, f64::max)
            - points.iter().copied().fold(f64::MAX, f64::min);
        assert!(span < 0.05, "0.01 of movement drew a span of {span}");

        let empty = read(unsafe { sg_sparkline_points(core, c("[]").as_ptr()) });
        assert_eq!(empty["ok"], serde_json::json!([]));

        unsafe { sg_core_free(core) };
    }

    /// The guard that makes this boundary safe.
    ///
    /// The C# side reads these objects by name. Nothing in the compiler connects the two, so a
    /// field added, renamed or removed in Rust would otherwise reach the Windows app as a silently
    /// missing value — the JSON stays valid, the property just never populates. Asserting the exact
    /// key set turns that into a failing test here, the way `crates/sg-sync` does for the pairing
    /// wire format. If this test fails, the fix is to update the C# record too, not to widen the
    /// assertion.
    #[test]
    fn field_set_is_asserted_so_a_new_field_fails_here() {
        let core = sg_core_new();
        let added = read(unsafe { sg_add_target(core, c(CONFIG).as_ptr()) });
        let id = added["ok"].as_str().expect("an id").to_string();
        let snapshot = read(unsafe { sg_snapshot(core, c(&id).as_ptr()) });
        let snapshot = &snapshot["ok"];

        assert_eq!(
            keys(snapshot),
            expected(&[
                "targetId",
                "state",
                "displayName",
                "distro",
                "kernel",
                "arch",
                "cpuCount",
                "gauges",
                "detailGroups",
                "entities",
                "topProcesses",
                "health",
                "simpleTiles",
                "sourceErrors",
                "lastUpdateMs",
                "roundTrips",
            ])
        );

        assert_eq!(
            keys(&snapshot["health"]),
            expected(&["level", "headline", "detail"])
        );

        unsafe { sg_core_free(core) };

        // The remaining records are asserted from a literal rather than a live host, because they
        // are only populated once a machine has answered and this suite must not need one.
        let gauge = crate::MetricGauge {
            series_id: "s".into(),
            metric: "cpu_usage".into(),
            label: "CPU".into(),
            value: 12.0,
            max: Some(100.0),
            unit_suffix: "%".into(),
            binary_scaled: false,
            history: vec![1.0, 2.0],
            severity: "ok".into(),
        };
        assert_eq!(
            keys(&serde_json::to_value(&gauge).expect("gauge serialises")),
            expected(&[
                "seriesId",
                "metric",
                "label",
                "value",
                "max",
                "unitSuffix",
                "binaryScaled",
                "history",
                "severity",
            ])
        );

        let tile = crate::SimpleTile {
            metric: "cpu_usage".into(),
            name: "Processor".into(),
            value_text: "12%".into(),
            summary: "Barely working".into(),
            fraction: Some(0.12),
            level: "ok".into(),
            history: vec![1.0],
        };
        assert_eq!(
            keys(&serde_json::to_value(&tile).expect("tile serialises")),
            expected(&[
                "metric",
                "name",
                "valueText",
                "summary",
                "fraction",
                "level",
                "history",
            ])
        );

        let process = crate::ProcessView {
            pid: "1".into(),
            command: "init".into(),
            cpu_percent: 1.0,
            memory_bytes: 2.0,
            state: "S".into(),
            machine_fraction: 0.01,
            severity: "ok".into(),
        };
        assert_eq!(
            keys(&serde_json::to_value(&process).expect("process serialises")),
            expected(&[
                "pid",
                "command",
                "cpuPercent",
                "memoryBytes",
                "state",
                "machineFraction",
                "severity",
            ])
        );

        let entity = crate::EntityView {
            id: "e".into(),
            kind: "cpu".into(),
            display: "0".into(),
            parent: None,
            gauges: vec![],
        };
        assert_eq!(
            keys(&serde_json::to_value(&entity).expect("entity serialises")),
            expected(&["id", "kind", "display", "parent", "gauges"])
        );
    }

    /// The fielded enum is the shape a C struct surface handles worst, so its encoding is pinned.
    #[test]
    fn connection_state_is_internally_tagged() {
        let idle = serde_json::to_value(ConnectionState::Idle).expect("serialises");
        assert_eq!(idle, serde_json::json!({ "kind": "idle" }));

        let failed = serde_json::to_value(ConnectionState::Failed {
            message: "no route to host".into(),
            recoverable: true,
        })
        .expect("serialises");
        assert_eq!(
            failed,
            serde_json::json!({
                "kind": "failed",
                "message": "no route to host",
                "recoverable": true,
            })
        );

        let reconnecting = serde_json::to_value(ConnectionState::Reconnecting {
            attempt: 3,
            retry_in_ms: 4000,
        })
        .expect("serialises");
        assert_eq!(
            reconnecting,
            serde_json::json!({ "kind": "reconnecting", "attempt": 3, "retryInMs": 4000 })
        );
    }

    /// Pairing hands out a handle, and a stale one is refused rather than dereferenced.
    #[test]
    fn a_forgotten_pairing_is_reported_not_dereferenced() {
        let core = sg_core_new();
        let gone = read(unsafe { sg_receiver_await_connection(core, 999) });
        assert_eq!(gone["err"]["kind"], "pairing");
        unsafe { sg_pairing_forget(core, 999) }; // harmless
        unsafe { sg_core_free(core) };
    }

    /// The merge rules are the security argument, so they are exercised across the boundary too.
    #[test]
    fn a_conflicting_pin_crosses_as_a_conflict_and_is_never_applied() {
        let core = sg_core_new();
        let existing = r#"{"hosts":[],"knownHosts":["a.test ssh-ed25519 AAAA"]}"#;
        let incoming = r#"{"hosts":[],"knownHosts":["a.test ssh-ed25519 BBBB"]}"#;

        let merged =
            read(unsafe { sg_merge_bundle(core, c(existing).as_ptr(), c(incoming).as_ptr()) });
        let merged = &merged["ok"];

        assert_eq!(merged["conflicts"].as_array().expect("conflicts").len(), 1);
        assert_eq!(merged["addedPins"], 0);
        assert_eq!(
            merged["knownHosts"],
            serde_json::json!(["a.test ssh-ed25519 AAAA"]),
            "the local pin wins; a sync channel that can quietly rewrite a pin is a \
             machine-in-the-middle with extra steps"
        );

        unsafe { sg_core_free(core) };
    }
}
