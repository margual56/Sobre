# Sobre

A small local email client: an action bar, a message list and a reader. Rust (Tauri 2) core with a SvelteKit UI.

<img width="1310" height="918" alt="final_demo" src="https://github.com/user-attachments/assets/4fadc512-54a5-43b7-a3f1-8d7c99775334" />

> [!IMPORTANT]
> This image is a demo, all info is made up and any resemblance to reality is purely coincidental.

## Run

```sh
npm install
npm run tauri dev      # development
npm run tauri build    # release binary and packages
cd src-tauri && cargo test
```

The tray icon needs an AppIndicator library. On Arch: `sudo pacman -S libayatana-appindicator`. Without it the app still works, but closing the window quits it.

## Signing in with Google

Google only lets mail apps in through OAuth, and each app must bring its own client ID. You create one once:

1. Open <https://console.cloud.google.com/>, create a project, and enable the **Gmail API** for it.
2. Under **Google Auth Platform → Audience**, choose **External** and add your own address as a user.
3. Still under **Audience**, set the publishing status to **In production**. In "Testing", Google expires your sign-in every 7 days. An unverified app in production shows a warning screen on sign-in; that is expected for a personal client.
4. Under **Data Access**, add the scope `https://mail.google.com/`.
5. Under **Clients**, create a client of type **Desktop app**. Copy its client ID and client secret.
6. In sobre, add your Gmail address and paste both values when asked.

If you would rather skip this, choose "Use an app password instead" and create one at <https://myaccount.google.com/apppasswords> (needs 2-step verification).

Microsoft accounts work the same way with an app registered in Entra ID (public client, redirect URI `http://127.0.0.1`, delegated permissions `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`). Leave the client secret empty.

## Where things are kept

Everything is in one SQLCipher database under `~/.local/share/dev.marcos.sobre/`: mail, attachments, the search index and cached icons. Next to it, `config.json` records only which key mode is in use and, in passphrase mode, the salt.

- **Wallet mode** keeps a random key in the system wallet (KWallet or GNOME Keyring), so the app opens and syncs unattended. Any program running as you can ask the wallet for that key.
- **Passphrase mode** derives the key from your passphrase with Argon2id and stores it nowhere.

Opening an attachment in another program writes a decrypted copy to `$XDG_RUNTIME_DIR`, removed when the app exits.

## How mail is rendered

Message HTML is sanitised in Rust, then shown in a sandboxed iframe (no scripts, no same-origin access) served from a separate `mailbody://` origin under a CSP that loads nothing but the app's own image endpoints. Remote images are off until you allow them and are then fetched by the Rust side, without cookies or a referrer and never from private network addresses. Links open in your browser only after you confirm the real destination.
