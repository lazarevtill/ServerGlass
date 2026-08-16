# Syncing across devices

Research and a recommendation. Nothing here is implemented yet.

The question — "sync my servers across my devices" — hides four separate decisions with opposite
risk profiles. Treating them as one is how sync features become the weakest part of an application.

## What there is to sync

| | Sensitivity | If it leaks | If it is wrong |
|---|---|---|---|
| **Host records** — address, port, user, auth method, refresh interval | Low. It is an inventory. | An attacker learns your server names. Unpleasant, not fatal. | You lose a list you can retype. |
| **Credentials** — passwords, key passphrases, pasted private keys | **Critical.** | Direct shell access to every server. | You cannot connect. |
| **Host key pins** — `known_hosts` | Low secrecy, **high integrity**. | Nothing; public keys are public. | **A silently replaced pin is exactly the machine-in-the-middle attack the pin exists to detect.** |
| **Preferences** — simple vs technical view | None. | Nothing. | Nothing. |

Two of these point in opposite directions:

- **Syncing credentials makes you less safe.** Every device and every sync hop is another place the
  key can leak from, and the blast radius is total.
- **Syncing host key pins makes you *more* safe.** A pin learned on your laptop protects your
  phone, which would otherwise trust-on-first-use all over again — and a phone on a hostile network
  is the likeliest place to meet an impostor.

So the honest answer to "sync everything" is: sync the inventory and the pins, and arrange for the
credentials not to need syncing at all.

## Threat model

Assume the sync server is hostile — you should design as though yours is, even though it is yours,
because a design that only holds while the server behaves is not a security design. Then:

1. **Server compromise.** The operator (or whoever took the box) must learn nothing beyond
   ciphertext sizes and timing.
2. **Device loss or theft.** A lost phone must be revocable without rotating credentials for every
   server, and without the thief being able to decrypt future syncs.
3. **Malicious sync content.** A compromised server must not be able to push a *changed host key
   pin* and have clients accept it silently. This is the one that most designs get wrong.
4. **Backup exfiltration.** iCloud/Google backups, laptop Time Machine, a stolen disk.
5. **Coercion / recovery.** If the user forgets the passphrase, what is lost, and can the operator
   help? (If the operator can help, the operator can also be compelled.)

## Options considered

### A. Platform-native keychain sync

Mark items `kSecAttrSynchronizable` and let iCloud Keychain carry them; use Block Store or the
Google Password Manager on Android.

**Verdict: cannot solve this problem.** Two blocking limits:

- iCloud Keychain syncs only within Apple's ecosystem — an Android phone is simply not in it, and
  cross-ecosystem is the actual requirement here.
- Apple's own documentation restricts synchronizable items to **password items**; certificates and
  cryptographic keys are not synced. A pasted SSH private key does not fit the supported shape.
- `kSecAttrSynchronizable` is mutually exclusive with the `ThisDeviceOnly` accessibility classes
  ServerGlass deliberately uses today, so enabling it is a downgrade of the current storage.

Worth keeping for what it is good at: Apple-to-Apple convenience for the *inventory*, not the keys.

### B. Encrypted blob in a file-sync service (iCloud Drive, Syncthing, Nextcloud, a git repo)

Encrypt the whole store with a passphrase, drop the file in whatever already syncs.

**Verdict: workable, and the cheapest thing that is not wrong** — but two sharp edges. There is no
per-device revocation (everyone with the passphrase can read everything, forever, including from
old copies), and file-level sync produces whole-file conflicts, which for a pin store means a
merge that can silently drop a pin.

### C. A small end-to-end encrypted sync service

Clients hold per-device keypairs; a record is encrypted to every trusted device. The server stores
ciphertext and an ordering, and learns nothing else. Adding a device requires an existing device to
approve it — the "trust circle" pattern that Apple, Matrix and Signal all converge on: prove
control of an already-trusted device before the circle expands.

**Verdict: the right shape if credentials must sync.** It gives real revocation: drop a device from
the circle, re-encrypt forward, and the removed device cannot read anything issued after — which is
precisely the property device revocation must have.

**Cost:** it is a protocol, a server, and a device-management UI. That is a large amount of
security-critical surface for an app whose whole premise is that it stores as little as possible.

### D. Stop having a credential to sync — per-device keys plus an SSH CA

Each device generates its **own** SSH keypair, in hardware where available (Secure Enclave on
Apple, StrongBox on Android), and that key **never leaves the device** — there is nothing to sync
and nothing to steal from a backup. Authorisation comes from a certificate authority signing that
device's public key, with a short lifetime.

You already run this infrastructure: `vault.in.lazarev.cloud`, with the CI templates authenticating
by JWT and no stored credential. Vault's SSH secrets engine is exactly this: the client sends its
**public** key, receives a signed certificate back, and each host trusts the CA via
`TrustedUserCAKeys` in `sshd_config`. Typical TTLs are minutes to hours, which is what makes
revocation infrastructure unnecessary — a leaked certificate expires before it is worth using.

**Verdict: the safest available answer, and the one your infrastructure is already shaped for.**

Two honest caveats:

- The client still holds a private key; the certificate does not replace it. What changes is that
  the key is per-device, hardware-backed, non-exportable and never synced, and the *authorisation*
  is short-lived and centrally revocable.
- A Secure Enclave key is ECDSA P-256, not Ed25519. OpenSSH accepts `ecdsa-sha2-nistp256`, so this
  works — but signing has to happen through the platform API rather than by handing bytes to russh,
  which is real implementation work in `sg-transport`.

## Recommendation

A staged plan, cheapest and safest first. Each stage stands alone.

### Stage 1 — sync the inventory and the pins, never the credentials

- Host records and `known_hosts` sync; passwords, passphrases and pasted keys do not.
- On a device that has the record but not the credential, the host appears in the list and asks for
  its credential once, then stores it in that device's own keystore.
- **Pins are append-only and conflicts are loud.** A new pin for an unknown host merges silently; a
  pin that *differs* from one already held is never auto-applied — it surfaces as the same "this
  server's identity is different from last time" that a live mismatch produces. A sync channel that
  can quietly rewrite a pin is a machine-in-the-middle with extra steps.
- Transport: option B is enough here. The inventory is low-sensitivity and the pins are public; both
  still get encrypted, but a compromised store is an inventory leak rather than a breach.

This removes the retyping that motivates syncing at all, and the worst case is bounded.

### Stage 2 — remove the credential from the problem

Add certificate auth: device-generated hardware-backed key, `POST` its public key to Vault, receive
a short-lived certificate, present both. Configure `TrustedUserCAKeys` on the hosts.

After this, "syncing credentials" is not a feature anyone needs, because there is no long-lived
credential on the device to sync. Revocation becomes a Vault role change instead of a key rotation
across every server.

### Stage 3 — only if Stage 2 cannot cover every host

Some machines will never trust a CA. For those, and only those, build option C: per-device keys, a
trust circle requiring approval from an existing device, forward re-encryption on revocation.

Do not build Stage 3 first. It is the most code, the most risk, and it becomes unnecessary for
every host that Stage 2 covers.

## What not to do

- **Do not sync credentials "temporarily, in plaintext, over your own network."** Own-network is
  not a security boundary; it is a preference about topology.
- **Do not let the sync server ever hold a decryptable secret**, even yours. A design that depends
  on the operator being honest cannot be audited, and you are the operator today but not
  necessarily tomorrow.
- **Do not auto-apply a changed host key pin from sync.** See above; this is the single most
  dangerous shortcut available in this feature.
- **Do not make a recovery escape hatch the operator can use.** Anything you can use to recover, an
  attacker with your server can use to read. If recovery is wanted, it belongs with the user — a
  written-down recovery phrase — not with the service.

## Sources

- [Apple — Secure keychain syncing](https://support.apple.com/guide/security/secure-keychain-syncing-sec0a319b35f/web)
- [Apple — `kSecAttrSynchronizable`](https://developer.apple.com/documentation/security/ksecattrsynchronizable)
- [iOS Keychain iCloud sync: synchronizable vs device-only](https://ptkd.com/journal/ios-keychain-icloud-sync-synchronizable)
- [HashiCorp Vault — Signed SSH certificates](https://developer.hashicorp.com/vault/docs/secrets/ssh/signed-ssh-certificates)
- [HashiCorp — Managing SSH access at scale with Vault](https://www.hashicorp.com/en/blog/managing-ssh-access-at-scale-with-hashicorp-vault)
- [Matrix — End-to-end encryption implementation guide](https://matrix.org/docs/matrix-concepts/end-to-end-encryption/)
- [Messenger end-to-end encryption overview (multi-device key handling)](https://engineering.fb.com/wp-content/uploads/2023/12/MessengerEnd-to-EndEncryptionOverview_12-6-2023.pdf)
