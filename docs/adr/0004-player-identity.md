# 0004. Player identity: device keys now, optional accounts later

- Status: Accepted
- Date: 2026-09-25

## Context

Player-hosted servers (ADR 0003) need identities that cannot be spoofed, for bans, allow-lists,
operator permissions, and abuse control of shared services (listing, TURN). Full accounts add
security, privacy (GDPR; COPPA, since voxel sandboxes attract under-13 players), recovery, and
App Store obligations (Sign in with Apple when offering third-party sign-in on iOS).

## Options considered

1. **No identity:** typed names only. Spoofable; weak moderation.
2. **Device keys:** each install generates a key pair; the public key is the player ID.
3. **Accounts** via the master server (OAuth providers or passkeys, likely a managed auth
   service).

## Decision

**Device keys now; accounts later as an additive layer.**

- Each install generates an **Ed25519** key pair on first run. The **public key is the player
  ID**. On the web it is a **non-extractable WebCrypto `CryptoKey`** stored in IndexedDB; in
  Electron/Capacitor it lives in the OS keychain/keystore where available.
- **Join handshake:** the server sends a random nonce bound to the session (and, for
  WebTransport/WebRTC, the connection's certificate/DTLS fingerprint); the client returns
  `pubkey`, `displayName`, and a signature over it. Servers key bans, allow-lists, ops, and
  saved player data by public key.
- Keys can be **exported/imported** (file or QR) to move identity between devices.
- No personal data is collected by default.
- **Later — accounts:** players sign in through the master server; the master issues a signed,
  short-lived **account attestation** binding one or more device keys to an account. Servers
  verify it offline with the master's published public keys. Servers choose **offline mode**
  (any device key) or **online mode** (key must carry a valid attestation). Accounts enable
  friends lists and invites, cross-device identity, and recovery.

## Consequences

- The identity format never changes: bans, permissions, and player data recorded against device
  keys keep working after accounts ship.
- Players who clear browser storage without exporting their key become new players (until
  accounts exist). The client should prompt to back up the key.
- Master-server features that cost money (TURN credentials, public listing) are rate-limited per
  key before accounts exist, and can require accounts later.
- When accounts are added: privacy policy, age handling (COPPA), data deletion, and Sign in with
  Apple on iOS become requirements.
