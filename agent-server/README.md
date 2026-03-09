# agent-server

A personal AI agent that runs on your Linux server and lets you chat from your iPhone via Telegram.

Built on [`ds-api`](../ds-api) — your Rust functions become AI tools with zero boilerplate.

---

## Architecture

```
iPhone (Telegram app)
  │
  │  HTTPS
  ▼
Telegram Bot API
  │
  │  POST /webhook  (your server must be reachable over HTTPS)
  ▼
agent-server (axum)
  │
  ├─ per-chat DeepseekAgent (conversation history preserved)
  └─ tools registered via #[tool]
```

Each Telegram chat gets its own `DeepseekAgent` instance with persistent conversation history. Send a message → agent replies. If you send a second message while the agent is still running tools, it is injected mid-loop via the interrupt channel and picked up before the next API turn.

---

## Prerequisites

- Rust toolchain (`cargo`)
- A server reachable over HTTPS (required by Telegram for webhooks)
  - Use a reverse proxy (nginx / caddy) with a TLS certificate, or
  - Use a service like [ngrok](https://ngrok.com) for local development
- A Telegram bot token from [@BotFather](https://t.me/BotFather)
- A DeepSeek API key from [platform.deepseek.com](https://platform.deepseek.com)

---

## Quick start

### 1. Configure

```bash
cp .env.example .env
```

Edit `.env`:

```env
TELEGRAM_TOKEN=123456789:AAxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
DEEPSEEK_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# Optional
PORT=8080
SYSTEM_PROMPT=You are a helpful personal assistant.
WEBHOOK_SECRET=<output of: openssl rand -hex 32>
```

### 2. Build

```bash
# From the workspace root
cargo build --release -p agent-server
```

The binary is at `target/release/agent-server`.

### 3. Register the Telegram webhook

Run this once after your server is reachable over HTTPS.  Replace the placeholders with your values:

```bash
curl -X POST "https://api.telegram.org/bot<TELEGRAM_TOKEN>/setWebhook" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://<YOUR_DOMAIN>/webhook",
    "secret_token": "<WEBHOOK_SECRET>"
  }'
```

Telegram will confirm with `{"ok":true,"result":true,"description":"Webhook was set"}`.

To verify it worked:

```bash
curl "https://api.telegram.org/bot<TELEGRAM_TOKEN>/getWebhookInfo"
```

### 4. Run

```bash
./target/release/agent-server
```

Or with environment variables inline:

```bash
TELEGRAM_TOKEN=... DEEPSEEK_API_KEY=... ./target/release/agent-server
```

---

## Deployment on Linux (systemd)

### 1. Copy the binary

```bash
sudo cp target/release/agent-server /usr/local/bin/agent-server
sudo chmod +x /usr/local/bin/agent-server
```

### 2. Create the environment file

```bash
sudo mkdir -p /etc/agent-server
sudo cp .env.example /etc/agent-server/.env
sudo nano /etc/agent-server/.env   # fill in your values
sudo chmod 600 /etc/agent-server/.env
```

### 3. Create the systemd unit

```bash
sudo nano /etc/systemd/system/agent-server.service
```

```ini
[Unit]
Description=Personal AI agent server
After=network.target

[Service]
Type=simple
User=agent
EnvironmentFile=/etc/agent-server/.env
ExecStart=/usr/local/bin/agent-server
Restart=on-failure
RestartSec=5
; Log level — set to debug for troubleshooting
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### 4. Enable and start

```bash
sudo systemctl daemon-reload
sudo systemctl enable agent-server
sudo systemctl start agent-server
sudo systemctl status agent-server
```

### 5. View logs

```bash
journalctl -u agent-server -f
```

---

## Reverse proxy (nginx)

Telegram requires HTTPS.  A minimal nginx config that terminates TLS and forwards to the agent server:

```nginx
server {
    listen 443 ssl;
    server_name your.domain.com;

    ssl_certificate     /etc/letsencrypt/live/your.domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/your.domain.com/privkey.pem;

    location /webhook {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

Get a free TLS certificate with [Certbot](https://certbot.eff.org):

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d your.domain.com
```

---

## Local development with ngrok

If you don't have a domain yet, ngrok creates a temporary public HTTPS URL that tunnels to your local machine:

```bash
# Install ngrok, then:
ngrok http 8080
```

ngrok prints a URL like `https://abc123.ngrok.io`. Use that as your webhook URL:

```bash
curl -X POST "https://api.telegram.org/bot<TOKEN>/setWebhook" \
  -d "url=https://abc123.ngrok.io/webhook"
```

Re-register the webhook each time ngrok restarts (the URL changes).

---

## Environment variables reference

| Variable | Required | Default | Description |
|---|---|---|---|
| `TELEGRAM_TOKEN` | ✅ | — | Bot token from @BotFather |
| `DEEPSEEK_API_KEY` | ✅ | — | DeepSeek API key |
| `PORT` | — | `8080` | Port the HTTP server listens on |
| `SYSTEM_PROMPT` | — | — | System prompt prepended to every conversation |
| `WEBHOOK_SECRET` | — | — | Secret token for webhook verification (recommended) |
| `RUST_LOG` | — | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

---

## Concurrency model

Each Telegram chat has one `DeepseekAgent` stored in a `Mutex<HashMap<chat_id, ChatEntry>>`. The agent is `take()`-n out of the map for the duration of a turn, so the lock is never held across an `.await`. Concurrent messages from the same chat are handled gracefully:

- If the agent is free, a new turn starts immediately.
- If the agent is busy (running tools), the new message is injected via the interrupt channel and picked up before the next API call — the user doesn't need to wait.