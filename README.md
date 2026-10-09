# Sobre

A small email client for the Linux desktop. Three panes and nothing else: an
action bar, your messages, and the one you are reading.

Sobre keeps your mail on your computer in an encrypted database, shows HTML
mail without letting it run or load anything, and tells you whether a message
really comes from who it claims.

<img width="1310" height="918" alt="final_demo" src="https://github.com/user-attachments/assets/4fadc512-54a5-43b7-a3f1-8d7c99775334" />

> [!IMPORTANT]
> This image is a demo, all info is made up and any resemblance to reality is purely coincidental.

## Install

Download the latest version from the
[releases page](https://github.com/margual56/Sobre/releases/latest).

| File | For |
|---|---|
| `.AppImage` | Any distribution. Make it executable and run it. |
| `.deb` | Debian, Ubuntu and relatives |
| `.rpm` | Fedora, openSUSE and relatives |

The AppImage looks for a new version when it starts and can update itself.
**Settings → Installation → Install** adds it to your application menu.

Linux on x86-64 only, for now.

Just curious? You can [try it without signing in](#try-it-without-signing-in).

## What it does

- **Several accounts in one inbox**, with search, and a switcher for accounts
  and folders.
- **Works with any IMAP provider.** Server settings are found from your address.
- **Reads HTML, markdown and plain text**, picking the right one per message.
- **Sender check on every message:** verified, unverified or failed, with the
  reasons a click away.
- **Spam and blocking:** report spam, or block an address or a whole domain.
- **One-click unsubscribe** when a newsletter offers it.
- **Runs in the background** with a tray icon and new-mail notifications.
- **Light and dark themes.**

## Privacy and security

**Your mail is encrypted on disk.** Everything Sobre stores (messages,
attachments, the search index, cached images) is in one encrypted database. You
choose how its key is kept:

- *System wallet* (default): the key sits in KWallet or GNOME Keyring, so Sobre
  opens by itself and checks mail in the background. This protects against a
  stolen disk, other users and backups. A program running as you could still
  ask the wallet for the key.
- *Passphrase*: the key is derived from a passphrase you type at every start
  and is stored nowhere. Nothing else on the computer can read your mail.

**Messages cannot run code or phone home.** HTML is cleaned and shown in a
sandbox with scripts off. Remote images are hidden until you allow them, so a
sender cannot tell that you opened a message; when you do allow them, Sobre
fetches them without cookies.

**Links ask first.** Clicking a link shows where it really goes, and warns when
the text names a different site than the destination.

**Forgery is flagged.** Sobre checks the sender's signature (DKIM) and the
domain's policy (DMARC) itself, and warns about lookalike domains, names that
show a different address, and replies that go somewhere else.

**Nothing leaves your computer except to your mail provider.** There is no
Sobre server and no telemetry. Two optional features contact other sites, and
both can be turned off in Settings: fetching sender icons from the sender's
website, and the update check against GitHub.

Opening an attachment in another program necessarily writes a decrypted copy;
it goes to a private in-memory folder and is removed when Sobre exits.

## How this was built, and why it trusts so little

Most of Sobre's code was written with an AI coding assistant (Claude), working
from my design and under my direction. I decided what it should do and how it
should behave.

That is one reason the design assumes as little as possible: no single piece
has to be right for you to stay safe, whoever or whatever wrote it. It is also
why nothing below asks you to take my word for it. Each claim links to the code
that implements it, the tests that exercise it, or the standard it follows.

You are still trusting the app itself with your mail and passwords, as with any
mail client, and Sobre has not had an independent security audit. Treat it as a
young project. Reviews and bug reports are very welcome.

## Check it yourself

### Where your password goes

Your password, app password or sign-in token is written in one place and read
in one place:

- **Stored** by [`accounts/secrets.rs`](src-tauri/src/accounts/secrets.rs): in the system
  wallet through the [Secret Service API](https://specifications.freedesktop.org/secret-service-spec/latest/),
  or inside the encrypted database in passphrase mode.
- **Read** only by `login_for` in [`accounts/mod.rs`](src-tauri/src/accounts/mod.rs),
  which hands it to exactly two callers: the IMAP sign-in in
  [`mail/imap.rs`](src-tauri/src/mail/imap.rs) and the SMTP sign-in in
  [`mail/smtp.rs`](src-tauri/src/mail/smtp.rs). Both connect, over TLS checked against
  your system's certificates, to the servers shown when you added the account.
- **Browser sign-in** follows the OAuth rules for desktop apps
  ([RFC 8252](https://www.rfc-editor.org/rfc/rfc8252), with PKCE,
  [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)). The only token
  endpoints are Google's and Microsoft's, written out in
  [`accounts/oauth.rs`](src-tauri/src/accounts/oauth.rs).

Searching the source for `secrets::get` shows there is no other reader.

### Every connection Sobre makes

This is the complete list. The interface cannot add to it (see the next table),
so the Rust files named here are the only code that opens a connection.

| Connects to | When | What is sent | Code |
|---|---|---|---|
| Your provider's IMAP and SMTP servers | Always | Your sign-in and your mail | [`mail/imap.rs`](src-tauri/src/mail/imap.rs), [`mail/smtp.rs`](src-tauri/src/mail/smtp.rs) |
| Google or Microsoft sign-in | Browser sign-in only | The OAuth exchange | [`accounts/oauth.rs`](src-tauri/src/accounts/oauth.rs) |
| `autoconfig.thunderbird.net` | Adding an account from an unknown provider | Your address's domain, never the address | [`accounts/discovery.rs`](src-tauri/src/accounts/discovery.rs) |
| Your DNS resolver | Adding an account; checking a sender | Lookups for the sender's domain (signature key, policy, logo) | [`trust/auth.rs`](src-tauri/src/trust/auth.rs), [`icons/mod.rs`](src-tauri/src/icons/mod.rs) |
| The sender's website | Showing a sender icon (can be turned off) | A request for its icon | [`icons/mod.rs`](src-tauri/src/icons/mod.rs) |
| Image hosts named in a message | Only after you click "Show images" | A plain request, no cookies or referrer | [`render/protocol.rs`](src-tauri/src/render/protocol.rs), [`net.rs`](src-tauri/src/net.rs) |
| The sender's unsubscribe address | Only when you click Unsubscribe | The one-click request ([RFC 8058](https://www.rfc-editor.org/rfc/rfc8058)) or an email | [`commands.rs`](src-tauri/src/commands.rs) |
| `api.github.com` and GitHub downloads | Update check at start, AppImage only (can be turned off) | Nothing about you | [`update.rs`](src-tauri/src/update.rs) |

Two details worth knowing: DNS lookups tell your resolver which domains write
to you, and if your system has no resolver configured Sobre falls back to
Cloudflare's over TLS ([`state.rs`](src-tauri/src/state.rs)).

### The claims, one by one

| Claim | How it is done | Where to look |
|---|---|---|
| The interface has no network access of its own | The window runs under a content security policy that allows connections only to the app's own core, and it is granted no plugin that can open one. It can only call the fixed list of commands the core registers. | Policy: `csp` in [`tauri.conf.json`](src-tauri/tauri.conf.json) ([CSP Level 3](https://www.w3.org/TR/CSP3/)). Grants: [`capabilities/default.json`](src-tauri/capabilities/default.json). Commands: `invoke_handler` in [`lib.rs`](src-tauri/src/lib.rs) |
| Message HTML cannot run code | Allow-list cleaning with [ammonia](https://github.com/rust-ammonia/ammonia), which parses HTML the way browsers do: only known-safe tags, attributes, URL schemes and CSS properties survive. Scripts, forms, frames, event handlers and `<style>` rules that could load anything are dropped. | [`render/sanitize.rs`](src-tauri/src/render/sanitize.rs); the hostile-input test `hostile_markup_is_removed` is at the bottom of that file |
| A mistake in the cleaning step alone is not enough | The cleaned message is shown in an [`<iframe sandbox>`](https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-sandbox) with no permissions (no scripts, no same-origin access), served from its own `mailbody://` origin with a policy of `default-src 'none'`. These three are enforced by the browser engine. | Sandbox: [`Reader.svelte`](src/lib/components/Reader.svelte). Origin and policy: [`render/protocol.rs`](src-tauri/src/render/protocol.rs), `BODY_CSP` in [`render/mod.rs`](src-tauri/src/render/mod.rs) |
| A message cannot load anything or reveal that you opened it | Remote images are removed during cleaning unless you allow them for that message. Allowed images are fetched by the core through a per-session secret path a message cannot guess, without cookies or referrer. | `attribute_filter` in [`render/sanitize.rs`](src-tauri/src/render/sanitize.rs); [`render/protocol.rs`](src-tauri/src/render/protocol.rs) |
| Mail cannot be used to reach your local network | Anything fetched because a message asked for it must resolve to a public address, and the checked address is pinned for the request. Redirects are followed by hand so each hop is checked. | `is_public` and `client_for` in [`net.rs`](src-tauri/src/net.rs), with tests |
| Links cannot open on their own | The window refuses to navigate anywhere a message points; the link is handed to the interface, which shows the real destination and waits for you. Links whose text names another site are flagged. | `allow_navigation` in [`lib.rs`](src-tauri/src/lib.rs); `find_deceptive_links` in [`render/sanitize.rs`](src-tauri/src/render/sanitize.rs) |
| A sender cannot fake its own verification | Signatures are verified locally with [mail-auth](https://github.com/stalwartlabs/mail-auth) (DKIM, [RFC 6376](https://www.rfc-editor.org/rfc/rfc6376)) and compared with the From domain and its published policy (DMARC, [RFC 7489](https://www.rfc-editor.org/rfc/rfc7489)). An `Authentication-Results` header ([RFC 8601](https://www.rfc-editor.org/rfc/rfc8601)) is believed only when it is the topmost one and was written by your own provider. | `verify`, `decide` and `authserv_is_trusted` in [`trust/auth.rs`](src-tauri/src/trust/auth.rs), with tests; lookalike checks in [`trust/heuristics.rs`](src-tauri/src/trust/heuristics.rs) |
| Your mail is encrypted on disk | One [SQLCipher](https://www.zetetic.net/sqlcipher/design/) database (AES-256, every page authenticated) holds everything, search index included. In passphrase mode the key comes from Argon2id ([RFC 9106](https://www.rfc-editor.org/rfc/rfc9106); 64 MiB, 3 passes) and is never stored. The window keeps no cache or storage on disk. | [`db/mod.rs`](src-tauri/src/db/mod.rs), [`db/key.rs`](src-tauri/src/db/key.rs); the test `file_is_encrypted_and_wrong_key_fails`; `incognito` in [`lib.rs`](src-tauri/src/lib.rs) |
| An update cannot swap in something else | The download must come from this repository's releases and match the size and SHA-256 GitHub publishes for it, or the old file is left alone. It is not signed by me, so this rests on GitHub and HTTPS. | `pick_update` and `put_in_place` in [`update.rs`](src-tauri/src/update.rs), with tests |
| What you download is built from this source | Releases are built by GitHub Actions from the tagged commit. The builds are not reproducible, so this is as trustworthy as the workflow file and GitHub. | [`.github/workflows/release.yml`](.github/workflows/release.yml) |

To run the tests yourself: `cd src-tauri && cargo test --lib`.

## Adding an account

Enter your address and Sobre finds the servers. Most providers then just need
your password, or an app password if you have two-step verification on.

### Gmail

Google does not accept your normal password in mail apps. The quick way:

1. Turn on 2-step verification for your Google account, if it is not on.
2. Create an app password at <https://myaccount.google.com/apppasswords>.
3. In Sobre, add your Gmail address, choose **Use an app password instead**,
   and paste it.

If you would rather sign in through the browser, so that Sobre never holds a
password, see [docs/google-oauth.md](docs/google-oauth.md). It needs a one-time
setup on Google's side. The same page covers Microsoft accounts.

## Try it without signing in

You do not need an email account to look around. Sobre comes with a demo
mailbox: a made-up account with sample messages, including a newsletter, a
receipt with an attachment and a phishing example that shows the warnings.

- **On first launch**, choose **Skip for now, open the demo account** in the
  add-account dialog.
- **Later**, use **Settings → Demo mailbox**, or start Sobre with `--demo`.

While the demo is open your own accounts are closed and nothing in them
changes. Sending and unsubscribing are switched off there. **Settings → Demo
mailbox → Back to my mail** returns you to your accounts. It is also handy for
screenshots.

## Troubleshooting

- **No tray icon.** Sobre shows a warning at start with the package to install
  for your distribution. Without the tray it quits when you close the window.
- **Mail is not arriving.** The action bar names the account it cannot reach;
  Settings shows the error from the server.

## Building from source

You need Rust, Node.js and the Tauri 2 system libraries
(`webkit2gtk-4.1`, `libayatana-appindicator`, `librsvg`, OpenSSL headers).

```sh
npm install
npm run tauri dev                 # run with live reload
npm run tauri build               # packages in src-tauri/target/release/bundle
cd src-tauri && cargo test --lib  # tests
```

The core is Rust (Tauri 2); the interface is SvelteKit.

Releases are cut by CI. Every push to `main` is read for
[conventional commits](https://www.conventionalcommits.org/): `feat` bumps the
minor version, `fix` and `perf` the patch, and anything else releases nothing.

## License

Apache-2.0. See [LICENSE](LICENSE).

The sender logos are trademarks of their owners and are shown only to identify
who a message is from.
