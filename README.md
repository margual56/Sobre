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

That is one reason the design assumes as little as possible. Sobre is built so
that no single piece has to be right for you to stay safe, whoever or whatever
wrote it:

- **Mail is treated as hostile.** Nothing a sender writes is trusted: not the
  HTML, not the links, not the images, not the headers claiming the message
  passed its checks. Only your own provider's verdict and signatures Sobre
  verifies itself count.
- **Several independent layers, not one filter.** A message is cleaned, then
  shown in a sandbox with scripts off, on an origin separate from the app,
  under a policy that lets it load nothing. The last three are enforced by the
  browser engine, not by Sobre's own code, so a mistake in the cleaning step
  alone does not let a message run code or reach the app.
- **The interface cannot reach the network.** Every connection is made by the
  Rust core, and anything fetched because a message asked for it is limited to
  public addresses and stripped of cookies.
- **No server to trust.** There is no Sobre account, backend or telemetry.
  Your mail goes between your computer and your provider, and is encrypted on
  disk.
- **Updates are checked before they replace anything.** A download that does
  not match the published checksum is discarded.

What this does not mean: you are still trusting the app itself with your mail
and passwords, as with any mail client. The security-sensitive parts (cleaning
HTML, the encrypted store, sender verification, the update check) have
automated tests you can read in `src-tauri/src`, but Sobre has not had an
independent security audit. Treat it as a young project. Reviews and bug
reports are very welcome, especially of those parts.

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

## Trying it without your mail

**Settings → Demo mailbox** swaps your mail for a made-up account with sample
messages, including a phishing example. Your own accounts are closed while it
is open. Handy for a look around, or for screenshots.

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
