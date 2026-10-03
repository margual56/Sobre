import { invoke } from "@tauri-apps/api/core";

export type KeyMode = "wallet" | "passphrase";
export type Stage = "loading" | "new" | "locked" | "ready";

export interface Status {
  stage: Exclude<Stage, "loading">;
  key_mode: KeyMode | null;
  has_tray: boolean;
}

export interface ServerConfig {
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  smtp_starttls: boolean;
  oauth_provider: string | null;
  source: string;
}

export interface Account {
  id: number;
  email: string;
  display_name: string;
  imap_host: string;
  auth_kind: string;
  oauth_provider: string | null;
}

export interface Folder {
  id: number;
  account_id: number;
  name: string;
  role: string;
}

export type Verdict = "verified" | "unverified" | "failed";

export interface MessageRow {
  id: number;
  account_id: number;
  folder_id: number;
  folder_role: string;
  subject: string;
  from_name: string;
  from_addr: string;
  date: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  has_attachments: boolean;
  preview: string;
  auth_verdict: Verdict | null;
}

export interface Contact {
  name: string;
  addr: string;
}

export interface AuthReport {
  verdict: Verdict;
  from_domain: string;
  dkim: { domain: string; selector: string; result: string; detail: string; aligned: boolean }[];
  dmarc_policy: string | null;
  provider: { authserv_id: string; spf: string | null; dkim: string | null; dmarc: string | null } | null;
  warnings: string[];
  summary: string;
}

export interface Attachment {
  index: number;
  name: string;
  mime: string;
  size: number;
}

export interface MessageDetail {
  row: MessageRow;
  extra: {
    to: Contact[];
    cc: Contact[];
    reply_to: string | null;
    message_id: string | null;
    references: string | null;
    size: number;
  };
  attachments: Attachment[];
  auth: AuthReport | null;
  first_time_sender: boolean;
  sender_blocked: boolean;
  images_trusted: boolean;
  kind: "html" | "markdown" | "plain";
  blocked_images: number;
  deceptive_links: string[];
  unsubscribe: { method: "one_click" | "mail" | "link"; target: string } | null;
  body_url: string;
}

export interface ListQuery {
  account_id: number | null;
  folder_id: number | null;
  role: string;
  search: string | null;
  offset?: number;
  limit?: number;
}

export interface Draft {
  account_id: number;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  body: string;
  markdown: boolean;
  in_reply_to: string | null;
  references: string | null;
  attachments: string[];
  reply_to_id: number | null;
}

export type Theme = "system" | "light" | "dark";

export interface Settings {
  notifications: boolean;
  fetch_icons: boolean;
  auto_junk_failed: boolean;
  autostart: boolean;
  theme: Theme;
}

export interface Stats {
  version: string;
  disk_bytes: number;
  messages: number;
  unread: number;
  downloaded: number;
  mail_bytes: number;
  image_count: number;
  image_bytes: number;
  icon_count: number;
  icon_bytes: number;
  blocked: number;
  periods: { label: string; received: number; sent: number; spam: number }[];
  top_senders: [string, string, number][];
  verdicts: [string, number][];
  accounts: [string, number][];
}

export interface OAuthClient {
  provider: string;
  client_id: string;
  client_secret: string;
}

export type BackendEvent =
  | { type: "mail_changed"; account_id: number }
  | { type: "message_updated"; id: number }
  | { type: "new_mail"; account_id: number; from: string; subject: string; count: number }
  | { type: "sync_status"; account_id: number; state: string; detail: string }
  | { type: "sent" }
  | { type: "theme_changed" }
  | { type: "locked" };

export const api = {
  status: () => invoke<Status>("app_status"),
  createStore: (passphrase: string | null) => invoke<void>("create_store", { passphrase }),
  unlock: (passphrase: string | null) => invoke<void>("unlock", { passphrase }),
  lock: () => invoke<void>("lock"),
  setKeyMode: (mode: KeyMode, passphrase: string | null) => invoke<void>("set_key_mode", { mode, passphrase }),

  discover: (email: string) => invoke<ServerConfig>("discover_account", { email }),
  addAccount: (account: {
    email: string;
    display_name: string;
    username: string | null;
    config: ServerConfig;
    password: string | null;
  }) => invoke<Account>("add_account", { account }),
  accounts: () => invoke<Account[]>("list_accounts"),
  removeAccount: (id: number) => invoke<void>("remove_account", { id }),
  getOAuthClient: (provider: string) => invoke<OAuthClient>("get_oauth_client", { provider }),
  setOAuthClient: (client: OAuthClient) => invoke<void>("set_oauth_client", { client }),

  folders: () => invoke<Folder[]>("list_folders", { accountId: null }),
  messages: (query: ListQuery) => invoke<MessageRow[]>("list_messages", { query }),
  message: (id: number) => invoke<MessageDetail>("get_message", { id }),
  quoteText: (id: number) => invoke<string>("quote_text", { id }),
  unsubscribe: (id: number) => invoke<string | null>("unsubscribe", { id }),
  senderIcon: (domain: string, verified: boolean) => invoke<string | null>("sender_icon", { domain, verified }),

  setFlag: (ids: number[], flag: "seen" | "flagged", on: boolean) => invoke<void>("set_flag", { ids, flag, on }),
  move: (ids: number[], role: "trash" | "junk" | "archive" | "inbox") => invoke<void>("move_messages", { ids, role }),
  block: (pattern: string) => invoke<string>("block_sender", { pattern }),
  unblock: (pattern: string) => invoke<void>("unblock_sender", { pattern }),
  blocked: () => invoke<string[]>("list_blocked"),
  trustImages: (addr: string, trusted: boolean) => invoke<void>("trust_images", { addr, trusted }),
  syncNow: () => invoke<void>("sync_now"),
  loadOlder: (folderId: number) => invoke<void>("load_older", { folderId }),
  openLink: (url: string) => invoke<void>("open_link", { url }),

  saveAttachment: (id: number, index: number) => invoke<string | null>("save_attachment", { id, index }),
  openAttachment: (id: number, index: number) => invoke<void>("open_attachment", { id, index }),
  pickFiles: () => invoke<string[]>("pick_files"),
  openComposeWindow: (draft: unknown) => invoke<void>("open_compose_window", { draft }),
  takeComposeDraft: <T>(id: number) => invoke<T | null>("take_compose_draft", { id }),
  closeWindow: () => invoke<void>("close_window"),
  send: (draft: Draft) => invoke<void>("send_message", { draft }),

  stats: () => invoke<Stats>("get_stats"),
  clearStorage: (what: "images" | "mail") => invoke<number>("clear_storage", { what }),
  settings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
};
