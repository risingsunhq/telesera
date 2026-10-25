# Telesera

A self-contained, unofficial Telegram client for rooted LG webOS televisions, developed and maintained by **Ennio Cuteri**.

The Rust engine runs **on the TV**. It talks directly to Telegram and streams media into a lightweight, original web interface. No companion server, Mac, Spark, Tailscale connection or cloud subscription is needed while using the app.

Telesera uses the Telegram API and is part of the Telegram ecosystem. It is not an official Telegram or LG application.

![Telesera interface showing fictional conversations](docs/images/chat.png)

## What it does

- Floating, iPad-inspired chat sidebar with a separate archive.
- Private conversations, groups, channels, bots and saved messages.
- Text messages and supported bot keyboard and callback buttons.
- Sticker sending from recent stickers, favorites and installed packs, with still previews for animated/video stickers.
- Embedded Telegram Mini Apps with a restricted web bridge.
- Channel live streams (RTMP) in the TV player, marked LIVE in the chat list.
- Video artwork and Play together, central player controls, 15-second seeking and saved playback positions.
- Magic Remote pointer and directional navigation, including text-entry handling.
- Profile pictures, readable normalization of decorative titles and a dim moving clock/date screensaver.
- Automatic TV-language selection: English, Italian, Spanish, French and German, with English as the fallback.

![English sticker picker with fictional conversations and original sample artwork](docs/images/stickers.png)

## Architecture

```text
Magic Remote → webOS application → local Rust engine → Telegram
                  video player ← bounded HTTP stream ← media download
```

The engine binds only to `127.0.0.1:3190`. A random local token protects the API, and browser origins are restricted. Account/session files remain in a private directory on the TV. The interface uses HTML, CSS and JavaScript without a frontend framework or build step; the engine uses Rust and grammers.

Third-party Mini Apps connect directly to their own HTTPS sites. They do not receive Telesera's local API token. Telegram files remain subject to the TV's hardware codec support; the engine does not transcode unsupported formats. See [playback compatibility and conversion options](docs/playback.md).

## Requirements

- A rooted LG webOS TV with an **ARM64 Linux kernel**, SSH access and webOS Homebrew startup hooks.
- Telegram API credentials for your own application from [my.telegram.org](https://my.telegram.org).
- A computer with Python 3, OpenSSH and [`@webos-tools/cli`](https://github.com/webos-tools/cli) for installation.
- A Linux ARM64 Docker host for building the engine, or a compatible release artifact.

The initial hardware target is the LG B5 / webOS 25. Other models must be verified separately. This project does not root a TV and does not require enabling LG Developer Mode on an already rooted TV.

## Build and install

```sh
git clone https://github.com/ECuteri/telesera.git
cd telesera
# On a Linux ARM64 Docker host:
./scripts/build-arm64.sh
```

On your installation computer, create a private JSON file **outside Git**:

```json
{"api_id": 12345, "api_hash": "YOUR_32_CHARACTER_TELEGRAM_API_HASH"}
```

Save it with permissions `0600`. Configure a trusted SSH host alias for your television, then run:

```sh
python3 scripts/install.py --host lg-tv --config /private/path/config.json
```

The installer requires a clean committed checkout, preserves the previous release and account directory, generates a unique local token, packages the webOS wrapper and starts the engine. It does not reboot the TV or change its audio/picture settings. Open **Telesera** on the TV and sign in with your Telegram phone number, code and two-step password if enabled.

The generated IPK contains a local access token: keep it private. Public release archives contain generic application files, not an account-specific IPK. Never share `private/`, sessions or deployment configuration.

## Development

```sh
cd engine
cargo check --locked
cargo build --locked
cargo fmt --check
```

For a local development instance, supply `--config /private/path/config.json` with `api_id`, `api_hash`, a random `token` of at least 32 characters, `data_dir`, `ui_dir` and an unused loopback `port`. Open `http://127.0.0.1:PORT/#token=TOKEN` in a private browser profile. Do not use a real account for message-writing tests without authorization.

The API contract is in [docs/contract.md](docs/contract.md). The backend also supports QR login endpoints for trusted account migration. The initial user interface provides phone/code/password sign-in.

## Limits

This is a focused TV client, not a complete replacement for the official Telegram apps. Calling, contacts/location sharing, payment flows and password-protected callbacks are not implemented. Mini Apps retain their own interface and may require the pointer. External Mini App popups are blocked because webOS can replace the application window; Telegram chat links stay inside Telesera. Playback inside a third-party frame cannot be inspected by the parent; the custom idle clock stays disabled while that frame is open.

The native LG screensaver is a system function. Any system-level integration is separate from the application's clock. The app does not change microphone, camera, brightness or volume settings. Iframe camera, microphone and location permissions are denied.

## Contributing and licensing

Contributions are welcome from everyone through issues and pull requests. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development checks, translations and screenshot guidelines. Direct write access is managed by the maintainer.

Code is licensed under MIT, copyright Ennio Cuteri. This is an independent implementation with its own history; see [source provenance](docs/provenance.md). Third-party libraries retain their [licenses and notices](THIRD_PARTY.md).

Keep changes focused, preserve remote/pointer accessibility and verify real playback and focus transitions when those paths change. Use fictional conversations in public screenshots and issues. For vulnerabilities, read [SECURITY.md](SECURITY.md).
