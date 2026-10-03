import { listen } from "@tauri-apps/api/event";
import { api, type Theme, type Account, type BackendEvent, type Folder, type KeyMode, type MessageDetail, type MessageRow, type Stage } from "./api";
import { errorText } from "./format";

export interface ComposeInit {
  accountId: number | null;
  to: string;
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  replyToId: number | null;
  // Carried over when a draft moves to its own window.
  bcc?: string;
  markdown?: boolean;
  files?: string[];
}

export const app = $state({
  stage: "loading" as Stage,
  keyMode: null as KeyMode | null,
  hasTray: false,
  accounts: [] as Account[],
  folders: [] as Folder[],
  messages: [] as MessageRow[],
  selectedId: null as number | null,
  detail: null as MessageDetail | null,
  detailError: "",
  loadingDetail: false,
  filter: { accountId: null as number | null, role: "inbox", folderId: null as number | null, search: "" },
  sync: {} as Record<number, { state: string; detail: string }>,
  toast: "",
  compose: null as ComposeInit | null,
  dialog: null as null | "settings" | "account" | "stats",
  link: null as string | null,
  theme: "system" as Theme,
  /** Whether the effective theme is dark, after resolving `system`. */
  dark: false,
  question: null as null | { text: string; ok: string; answer: (yes: boolean) => void },
});

/** Ask a yes/no question in the app's own dialog. The webview's `confirm()`
 *  is not usable here: it returns at once without waiting for an answer. */
export function ask(text: string, ok = "OK"): Promise<boolean> {
  return new Promise((resolve) => {
    app.question = {
      text,
      ok,
      answer: (yes) => {
        app.question = null;
        resolve(yes);
      },
    };
  });
}

const systemDark = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;

export function applyTheme(theme: Theme) {
  app.theme = theme;
  app.dark = theme === "dark" || (theme === "system" && !!systemDark?.matches);
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

/** Read the saved theme; only possible once the store is unlocked. */
export async function loadTheme() {
  try {
    applyTheme((await api.settings()).theme);
  } catch {
    applyTheme("system");
  }
}

if (systemDark) {
  app.dark = systemDark.matches;
  systemDark.addEventListener("change", () => applyTheme(app.theme));
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function toast(text: string) {
  app.toast = text;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (app.toast = ""), 5000);
}

/** Run an action and surface any failure instead of losing it. */
export async function attempt<T>(work: () => Promise<T>): Promise<T | undefined> {
  try {
    return await work();
  } catch (e) {
    toast(errorText(e));
    return undefined;
  }
}

export async function refreshStatus() {
  const status = await api.status();
  app.stage = status.stage;
  app.keyMode = status.key_mode;
  app.hasTray = status.has_tray;
  if (status.stage === "ready") {
    await loadTheme();
    await loadEverything();
  }
}

export async function loadEverything() {
  app.accounts = await api.accounts();
  app.folders = await api.folders();
  await loadMessages();
  if (app.accounts.length === 0 && app.dialog === null) app.dialog = "account";
}

let listToken = 0;

export async function loadMessages() {
  const token = ++listToken;
  const rows = await api.messages({
    account_id: app.filter.accountId,
    folder_id: app.filter.folderId,
    role: app.filter.role,
    search: app.filter.search.trim() || null,
    limit: 500,
  });
  if (token !== listToken) return;
  app.messages = rows;
  if (app.selectedId !== null && !rows.some((r) => r.id === app.selectedId)) {
    app.selectedId = null;
    app.detail = null;
  }
}

export async function select(id: number | null) {
  app.selectedId = id;
  app.detailError = "";
  app.link = null;
  if (id === null) {
    app.detail = null;
    return;
  }
  app.loadingDetail = true;
  try {
    const detail = await api.message(id);
    if (app.selectedId !== id) return;
    app.detail = detail;
    if (!detail.row.seen) {
      const row = app.messages.find((m) => m.id === id);
      if (row) row.seen = true;
      await api.setFlag([id], "seen", true);
    }
  } catch (e) {
    if (app.selectedId === id) {
      app.detail = null;
      app.detailError = errorText(e);
    }
  } finally {
    if (app.selectedId === id) app.loadingDetail = false;
  }
}

export function selectedRow(): MessageRow | null {
  return app.messages.find((m) => m.id === app.selectedId) ?? null;
}

/** Move the selection by one row; used after deleting and by j/k. */
export function step(delta: number) {
  const i = app.messages.findIndex((m) => m.id === app.selectedId);
  const next = app.messages[i < 0 ? 0 : i + delta];
  if (next) void select(next.id);
}

/** Remove the selected message from view and pick its neighbour. */
async function removeSelected(action: (id: number) => Promise<unknown>) {
  const row = selectedRow();
  if (!row) return;
  const i = app.messages.indexOf(row);
  const neighbour = app.messages[i + 1] ?? app.messages[i - 1] ?? null;
  const done = await attempt(async () => {
    await action(row.id);
    return true;
  });
  if (!done) return;
  app.messages = app.messages.filter((m) => m.id !== row.id);
  await select(neighbour?.id ?? null);
}

export const actions = {
  toggleRead: () =>
    attempt(async () => {
      const row = selectedRow();
      if (!row) return;
      row.seen = !row.seen;
      await api.setFlag([row.id], "seen", row.seen);
    }),
  toggleStar: () =>
    attempt(async () => {
      const row = selectedRow();
      if (!row) return;
      row.flagged = !row.flagged;
      await api.setFlag([row.id], "flagged", row.flagged);
    }),
  remove: () => removeSelected((id) => api.move([id], "trash")),
  archive: () => removeSelected((id) => api.move([id], "archive")),
  spam: () => removeSelected((id) => api.move([id], "junk")),
  notSpam: () => removeSelected((id) => api.move([id], "inbox")),
  block: async (domain: boolean) => {
    const row = selectedRow();
    if (!row) return;
    const pattern = domain ? row.from_addr.slice(row.from_addr.lastIndexOf("@") + 1) : row.from_addr;
    const blocked = await attempt(() => api.block(pattern));
    if (blocked) {
      toast(`Blocked ${blocked}. Its mail now goes to Spam.`);
      app.selectedId = null;
      app.detail = null;
      await loadMessages();
    }
  },
};

function ownAddresses(): string[] {
  return app.accounts.map((a) => a.email.toLowerCase());
}

function quoted(text: string): string {
  return text.trimEnd().split("\n").map((line) => (line.startsWith(">") ? ">" + line : "> " + line)).join("\n");
}

export async function startCompose(kind: "new" | "reply" | "replyAll" | "forward") {
  const detail = app.detail;
  if (kind === "new" || !detail) {
    app.compose = { accountId: app.filter.accountId ?? app.accounts[0]?.id ?? null, to: "", cc: "", subject: "", body: "", inReplyTo: null, references: null, replyToId: null };
    return;
  }
  const { row, extra } = detail;
  const text = (await attempt(() => api.quoteText(row.id))) ?? "";
  const when = new Date(row.date * 1000).toLocaleString();
  const who = row.from_name ? `${row.from_name} <${row.from_addr}>` : row.from_addr;
  const bare = row.subject.replace(/^((re|fwd?|rv|fw)\s*:\s*)+/i, "");

  if (kind === "forward") {
    const header = `---------- Forwarded message ----------\nFrom: ${who}\nDate: ${when}\nSubject: ${row.subject}\nTo: ${extra.to.map((c) => c.addr).join(", ")}\n\n`;
    app.compose = { accountId: row.account_id, to: "", cc: "", subject: `Fwd: ${bare}`, body: `\n\n${header}${text}`, inReplyTo: null, references: null, replyToId: null };
    return;
  }
  const mine = ownAddresses();
  const primary = extra.reply_to ?? row.from_addr;
  const others = kind === "replyAll" ? [...extra.to, ...extra.cc].map((c) => c.addr).filter((a) => !mine.includes(a.toLowerCase()) && a.toLowerCase() !== primary.toLowerCase()) : [];
  app.compose = {
    accountId: row.account_id,
    to: primary,
    cc: [...new Set(others)].join(", "),
    subject: `Re: ${bare}`,
    body: `\n\nOn ${when}, ${who} wrote:\n${quoted(text)}\n`,
    inReplyTo: extra.message_id,
    references: extra.references,
    replyToId: row.id,
  };
}

let reloadTimer: ReturnType<typeof setTimeout> | undefined;

function scheduleReload() {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(async () => {
    if (app.stage !== "ready") return;
    await attempt(async () => {
      app.folders = await api.folders();
      await loadMessages();
    });
  }, 250);
}

export async function startListening() {
  await listen<BackendEvent>("backend", ({ payload }) => {
    switch (payload.type) {
      case "mail_changed":
        scheduleReload();
        break;
      case "message_updated":
        scheduleReload();
        if (payload.id === app.selectedId && app.detail && !app.detail.auth) {
          void api.message(payload.id).then((d) => {
            if (app.selectedId === payload.id) app.detail = d;
          });
        }
        break;
      case "sync_status":
        app.sync[payload.account_id] = { state: payload.state, detail: payload.detail };
        break;
      case "theme_changed":
        void loadTheme();
        break;
      case "sent":
        toast("Message sent.");
        break;
      case "locked":
        app.stage = "locked";
        app.messages = [];
        app.detail = null;
        app.selectedId = null;
        app.compose = null;
        break;
    }
  });
  await listen<string>("link-clicked", ({ payload }) => (app.link = payload));
  await listen("compose", () => app.stage === "ready" && startCompose("new"));
}
