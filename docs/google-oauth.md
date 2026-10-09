# Signing in with Google or Microsoft through the browser

Most people do not need this page. The quick way to add a Gmail account is an
[app password](../README.md#gmail). Follow this guide only if you prefer the
"Sign in with Google" browser flow, where Sobre never sees a password.

## Why there are steps at all

Google lets a mail app in through OAuth only if the app identifies itself with a
client ID. Sobre does not ship one: for full mailbox access Google requires the
app's publisher to pass a paid yearly security audit, and until then limits a
shared client to 100 users behind a warning screen. So you create a client of
your own, once. It is free and takes about five minutes.

## Google

1. Open <https://console.cloud.google.com/>, create a project, and enable the
   **Gmail API** for it.
2. Under **Google Auth Platform → Audience**, choose **External** and add your
   own address as a user.
3. Still under **Audience**, set the publishing status to **In production**. In
   "Testing", Google expires your sign-in every 7 days. An unverified app in
   production shows a warning screen when you sign in; that is expected for a
   personal client.
4. Under **Data Access**, add the scope `https://mail.google.com/`.
5. Under **Clients**, create a client of type **Desktop app**. Copy its client
   ID and client secret.
6. In Sobre, add your Gmail address and paste both values when asked. They are
   stored in the encrypted database and reused for every Google account you add.

## Microsoft

Register an app in Entra ID as a public client with the redirect URI
`http://127.0.0.1` and the delegated permissions `IMAP.AccessAsUser.All`,
`SMTP.Send` and `offline_access`. Paste its application ID as the client ID and
leave the client secret empty.
