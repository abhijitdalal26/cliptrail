# Self-hosting the ClipTrail relay

The relay lets your devices sync when they're **not on the same network**:
phone on mobile data, laptop at home (or asleep). It's a small mailbox
(`clipd-relay`, one Rust binary + one SQLite file) that stores only
**end-to-end-encrypted** envelopes and images. It never has a key that can
open them. Why a relay and not P2P: [`docs/adr/0001-off-lan-sync.md`](../docs/adr/0001-off-lan-sync.md).

What the relay can see: an opaque id per space, device ids and their
*public* keys, the sizes and times of encrypted events, an opaque per-clip
tag (so it can drop superseded events), the event type (add / pin / delete /
config), and client IPs. It can't see clip text, images, app names, device
names or your space key.

## Option A: a VPS or home server with a domain (Caddy, automatic HTTPS)

Needs: Docker with Compose, a DNS name (`relay.example.com`) pointing at the
machine, ports 80 and 443 open.

```sh
git clone https://github.com/abhijitdalal26/cliptrail && cd cliptrail/docker
cp relay.env.example .env
# edit .env: RELAY_DOMAIN=relay.example.com, RELAY_INVITE=$(openssl rand -base64 24)
docker compose up -d --build
curl https://relay.example.com/v1/hello      # → {"proto":1,"kind":"clipd-relay",...}
```

Caddy gets and renews the TLS certificate. Data lives in the `relay-data`
volume (back it up if you like; it's ciphertext only).

## Option B: Raspberry Pi at home

Same as A (the image builds for arm64; the first build takes a while on a
Pi 4, ~20-30 min). Forward ports 80 and 443 from your router to the Pi and
point a DNS name (a free dynamic-DNS name works) at your home IP. If you
can't open ports (CGNAT, ISP blocks), use option C.

## Option C: Cloudflare Tunnel (no open ports)

1. Cloudflare dashboard → Zero Trust → Networks → Tunnels → create a tunnel,
   copy its token.
2. Add a public hostname, e.g. `relay.example.com` → service `http://relay:8787`.
3. In `docker/.env` set `RELAY_DOMAIN=relay.example.com`, `RELAY_INVITE=...`
   and `CLOUDFLARE_TUNNEL_TOKEN=...`, then:

```sh
docker compose --profile tunnel up -d --build relay cloudflared
```

Cloudflare terminates TLS (so it sees the same ciphertext the relay sees,
nothing more). WebSockets work on the free plan; uploads are capped at
100 MB per request there, above the relay's 48 MB image limit.

## Without Docker

```sh
cargo build --release -p clipd-relay
CLIPD_RELAY_INVITE=... ./target/release/clipd-relay --bind 127.0.0.1:8787 --data-dir /var/lib/clipd-relay
```

Put any TLS reverse proxy in front (it must pass WebSocket upgrades), and set
`CLIPD_RELAY_TRUST_PROXY=true` so rate limits see real client IPs.

## Connect your devices

On the device that hosts the hub (the desktop app), or any paired device:

```sh
clipd sync relay check https://relay.example.com
clipd sync relay add https://relay.example.com --invite <RELAY_INVITE>
clipd sync relay status
```

The relay address and a space enroll key travel to your other devices
inside the encrypted sync channel, so they pick it up the next time they
sync with the hub on your home network; after that they use the relay
whenever the hub is out of reach. (The first pairing of a new device still
needs both devices on the same network.) `clipd sync relay remove <url>
[--purge]` stops using it everywhere; `--purge` also deletes your data from
the relay.

Internet relays must be `https://`. Plain `http://` is accepted only for
localhost and private LAN addresses (testing).

## Settings

Every flag has an environment variable (`clipd-relay --help`):

| Variable | Default | |
|---|---|---|
| `CLIPD_RELAY_BIND` | `0.0.0.0:8787` (image) | listen address |
| `CLIPD_RELAY_DATA` | `/data` (image) | where `relay.db` lives |
| `CLIPD_RELAY_INVITE` | – | invite code required to register a space |
| `CLIPD_RELAY_CLOSED` | `false` | refuse new spaces entirely |
| `CLIPD_RELAY_MAX_SPACE_MB` | 256 | quota per space; oldest data is evicted beyond it |
| `CLIPD_RELAY_MAX_BLOB_MB` | 48 | largest encrypted image |
| `CLIPD_RELAY_MAX_SPACES` | 100 | spaces per relay |
| `CLIPD_RELAY_MAX_DEVICES` | 16 | devices per space |
| `CLIPD_RELAY_RETENTION_DAYS` | 30 | events older than this are dropped (0 = never) |
| `CLIPD_RELAY_TRUST_PROXY` | `false` | use `X-Forwarded-For` for rate limits |
| `CLIPD_RELAY_DEVICE_RATE` / `CLIPD_RELAY_IP_RATE` | 1200 / 60 | requests per minute per device / per IP (unauthenticated or failed) |
| `CLIPD_RELAY_LOG` | `info` | log filter; logs never contain clip data, keys or signatures |

A device that was offline for longer than the retention period misses
deletes and pins older than that; its own clips are unaffected.
