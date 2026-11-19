# Agentero self-hosted bridge relay

Minimal Paseo-compatible WebSocket relay for Agentero's device Bridge. It routes
encrypted frames by `serverId`; it never sees plaintext Vault or Agent content.

## Endpoint

- Public: `wss://de.rune.ing:2054/ws`
- Health: `https://de.rune.ing:2054/healthz`
- Local upstream: `127.0.0.1:47821`

The desktop app defaults to `de.rune.ing:2054`. A pairing offer embeds the
endpoint, so mobile clients pick it up from the QR code.

## Protocol

- Host control channel: `/ws?v=2&serverId=agt_...&role=server`
- Client channel: `/ws?v=2&serverId=agt_...&role=client`
- Host data channel: `/ws?v=2&serverId=agt_...&role=server&connectionId=...`

The relay pairs the client with the host data channel and forwards text/binary
frames unchanged. Control messages use `sync`, `connected`, and
`disconnected` with camelCase `connectionIds` / `connectionId`.

## Install on Debian

```bash
useradd --system --home /opt/agentero-relay --shell /usr/sbin/nologin agentero-relay
install -d -m 755 /opt/agentero-relay
# copy relay.mjs and package.json into /opt/agentero-relay
cd /opt/agentero-relay
npm install --omit=dev
install -m 644 agentero-relay.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now agentero-relay
```

Caddy terminates TLS and proxies WebSocket upgrades:

```caddyfile
https://de.rune.ing:2054 {
    tls /etc/caddy/tls/de-rune-ing/fullchain.pem /etc/caddy/tls/de-rune-ing/privkey.pem
    reverse_proxy 127.0.0.1:47821
}
```

Allow the public port:

```bash
ufw allow 2054/tcp comment 'xuan-agentero-relay'
```

## Verify

```bash
curl -sS https://de.rune.ing:2054/healthz
node test-relay.mjs   # local protocol test
```

`test-relay.mjs` starts the relay on a random local port and checks control,
client pairing, text forwarding, binary forwarding, and disconnect messages.

