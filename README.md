# mail-receiver-rs

A simple [Delivery SMTP server](https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.10) for development and testing of software that requires email accounts. No accounts to manage - every unique recipient address is automatically accepted. Incoming emails are parsed, saved to disk, and can be browsed via a REST API and a built-in web UI.

## Features

- **SMTP server** - Full RFC 5321 support (EHLO/HELO, MAIL FROM, RCPT TO, DATA, RSET, NOOP, QUIT, pipelining, 8BITMIME, SMTPUTF8, dot-stuffing)
- **Email parsing** - Automatic extraction of headers, text/HTML body, attachments, and sender/recipient addresses via `mail-parser`
- **Recipient validation** - Optional domain whitelist and prefix-based spam filtering
- **File storage** - Every accepted email saved as raw bytes (`.raw`) and parsed JSON (`.json`), organized by recipient. Weekly index files for fast browsing
- **REST Admin API** - Query stored emails by week, recipient, or message ID; fetch raw messages and download attachments. Scoped API keys for authentication
- **Web UI** - A three-pane mail reader (React + TanStack Router + TanStack Query + Tailwind CSS + shadcn/ui) for browsing recipients/weeks, viewing messages, and previewing/downloading attachments. Built into the Docker image and served by the backend
- **Multi-arch Docker images** - Linux amd64, arm64, and armv7

## Requirements

- A server reachable via the public internet
- A domain name and access to its DNS configuration

## Setup

Let's assume you want to receive emails sent to `@dev1.mail.example.com`.

### DNS

```
mydevserver.example.com   A    <ip of your server>
dev1.mail.example.com     MX   10   mydevserver.example.com
```

### System Configuration

SMTP delivery runs on port 25. On Linux, only root can bind to this port.

### App Configuration

#### 1. Create a `.env` file

```
API_KEY=averylongpasswordtoprotectadminapi
EMAIL_DOMAIN=dev1.mail.example.com
EMAIL_ACCOUNT_PREFIX=goodmailonly-
ADMIN_APP_PORT=2255
```

Emails sent to addresses that don't start with the prefix will be ignored. In the above example, an email sent to `goodmailonly-test-MCPNBoXE@dev1.mail.example.com` will be saved; `test-MCPNBoXE@dev1.mail.example.com` will be rejected.

If `EMAIL_DOMAIN` or `EMAIL_ACCOUNT_PREFIX` are unset, all emails are accepted.

#### Scoped API keys

Instead of a single `API_KEY`, you can set `API_KEYS` with per-domain scopes:

```
API_KEYS=adminkey:*,devkey:dev1.mail.example.com
```

- `adminkey:*` - access to all domains
- `devkey:dev1.mail.example.com` - access only to `dev1.mail.example.com` messages

#### 2. Run with Docker

```
docker run -p 25:25 -p 2255:2255 -v $(pwd)/.env:/.env -v $(pwd)/mail:/mail darktohka/mail-receiver-rs
```

## API

All endpoints require the `api_key` query parameter.

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/api/mail` | Redirect to current ISO week |
| GET | `/api/week/{year}/{week}` | List messages in a week |
| GET | `/api/recipients` | List all recipient addresses with message counts |
| GET | `/api/weeks` | List available weekly index files |
| GET | `/api/domain/{domain}/{name}` | List messages for a recipient |
| GET | `/api/domain/{domain}/{name}/{filename}` | Fetch a parsed message JSON |
| GET | `/api/message/{message_id}` | Fetch a parsed message by UUID |
| GET | `/api/message/{message_id}/raw` | Fetch the raw RFC 822 bytes |
| GET | `/api/message/{message_id}/attachment/{index}` | Download or view an attachment (`?view=1`) |

Example: `GET /api/week/2025/29?api_key=yourkey`

## Web UI

The frontend is served at the `ADMIN_APP_PORT` alongside the API. Navigate to `http://yourserver:2255/` and enter your API key to log in.

- **Recipients view** - Browse all unique recipient addresses, see their messages, and open any message in the detail pane
- **Weekly view** - Browse by ISO week, filter messages, and open messages in the detail pane
- **Message detail** - Tabbed view showing rendered HTML (or plain text) and raw source; download or preview attachments (images, PDFs, text)

## Development

### Backend (Rust)

```
cd backend
cargo build
cargo test
cargo run
```

### Frontend

```
cd frontend
npm install
npm run dev    # dev server, proxies /api to the backend
npm run build  # production build -> frontend/dist/
```

### Tools

- **SMTP fuzzer** - `cd backend/tools/fuzzer && cargo run` - sends 20+ test scenarios to a running SMTP server
- **Raw email sender** - `cd backend/tools/raw-sender && cargo run [directory]` - replays saved `.raw` files via SMTP

## Configuration

All configuration is via environment variables (loaded from `.env`):

| Variable | Default | Description |
|----------|---------|-------------|
| `API_KEY` | required | Single wildcard API key (min 8 chars) |
| `API_KEYS` | - | Comma-separated `key:scope` pairs (overrides `API_KEY`) |
| `EMAIL_DOMAIN` | all | Comma-separated list of accepted domains |
| `EMAIL_ACCOUNT_PREFIX` | none | Required prefix for recipient local-parts |
| `ADMIN_APP_PORT` | disabled | Port for the Admin API + Web UI |
| `SMTP_PORT` | 25 | SMTP listen port |
