# ADR 0001: Off-LAN sync through a self-hostable ciphertext relay

- Status: accepted (implemented in `crates/clipd-relay`, `clipd_sync::relay*`); shipping in v1 is decided separately
- Date: 2026-11-01

## Context

Sync v1 is a LAN hub inside the desktop app: the phone syncs only while the
laptop is on and on the same Wi-Fi. We want sync across networks (phone on
mobile data, laptop at home or asleep) without weakening end-to-end
encryption. Sync envelopes (`{event_id, device_id, ts, nonce, ct}`) were
designed to be storage-agnostic for exactly this.

## Options

| | (a) Self-hostable "dumb" relay | (b) P2P (iroh-style) + relay fallback | (c) Bring-your-own S3/WebDAV mailbox |
|---|---|---|---|
| Laptop off / asleep | **Works**: the relay holds the log | Fails unless a relay also stores (then it is (a) plus NAT traversal) | Works |
| Setup for non-technical users | One Docker command + domain (or a hosted instance later: zero setup) | Zero for P2P, but the store-and-forward part still needs a server | Bucket, keys, CORS/permissions, per provider; no live push |
| What the server sees | Ciphertext, opaque ids, sizes, times, IPs, event type, per-clip opaque tag | Relay sees the same; peers see IPs | Provider sees object names, sizes, times |
| Ops cost | Tiny (Rust binary + SQLite, runs on a Pi) | Relay infra + NAT traversal quirks (CGNAT, mobile carriers) | None for us; user pays the provider |
| Fit with the envelope format | Direct: the relay stores the same envelopes the hub stores; same endpoints | Needs a new transport and session layer | Needs object layout, listing/polling, conflicts on seq assignment |
| Android battery / background | One WebSocket, only while the LAN hub is unreachable; catch-up is one GET | Keeping a P2P endpoint reachable in the background is costly; hole punching on mobile is flaky | Polling only (no push); many small requests |
| Future hosted (paid) option | Same binary run by us | Hosted relay anyway | Not applicable |

## Decision

**(a)**: `clipd-relay`, a self-hostable, ciphertext-only relay. Keep the LAN
hub as the fast path: devices use the hub when it answers and the relay
otherwise. A hosted instance later is the same binary run by us.

Design points:

- **Relay auth without shared secrets.** Each device has its own Ed25519
  key; the relay stores only public keys. Every request is signed over
  method, path, time, a random nonce and the body hash (`CTR1` header); the
  relay refuses repeats inside the ±2 min window. A space is registered by
  the first device together with a space *enroll key*; the enroll key travels
  to other devices inside a sealed `SpaceConfig` event, and each device
  enrolls its own key with it. Revoking a device on the hub deletes its key
  on the relay and rotates the enroll key.
- **The relay never decrypts.** Envelopes and image blobs are sealed on the
  devices (XChaCha20-Poly1305). The relay space id and the per-clip
  compaction tag are keyed MACs, not the space id or content hashes.
- **Hub bridge.** The hub forwards its whole log to the relay and pulls
  relay events into its log, so LAN devices and off-LAN devices see each
  other and a phone sees laptop clips after the laptop sleeps.
- **Clients** use the relay only while the hub is unreachable. Events are
  idempotent by `event_id`; devices remember seen ids so a double path
  doesn't double-apply.
- **Limits** on the relay: per-space quota (oldest evicted), blob size,
  devices per space, spaces per relay, retention (default 30 days),
  per-device and per-IP rate limits, invite code for registration.
- **Transport**: HTTPS required for internet relays (plain HTTP only for
  loopback/private addresses). Clients use rustls (ring) with Mozilla roots
  (webpki-roots), so Android needs no platform glue.

## Consequences

- The relay operator learns metadata: when and how much you copy, from which
  IPs, which events concern the same clip, and event types. Not content.
- The first pairing of a new device still needs the same network (the
  pairing exchange goes to the hub). Pairing through a relay mailbox is
  possible later without changing the envelope format.
- Retention means a device offline longer than N days misses old deletes and
  pins.
- Revocation is only as fast as the hub's next contact with the relay; there
  is still no space-key rotation, so a revoked device keeps what it already
  had (same as the LAN hub).
- Option (b) stays possible later as an optimisation for the both-online
  case; (c) can be added as another "relay" backend if users ask.
