export function shortDate(seconds: number): string {
  if (!seconds) return "";
  const d = new Date(seconds * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString([], sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
}

export function longDate(seconds: number): string {
  if (!seconds) return "";
  return new Date(seconds * 1000).toLocaleString([], { dateStyle: "full", timeStyle: "short" });
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function senderLabel(name: string, addr: string): string {
  return name.trim() || addr || "(unknown sender)";
}

export function initials(name: string, addr: string): string {
  const source = name.trim() || addr;
  const words = source.split(/[\s._@-]+/).filter(Boolean);
  return ((words[0]?.[0] ?? "?") + (words.length > 1 && name.trim() ? words[1][0] : "")).toUpperCase();
}

/** A stable hue per sender, so the same person always gets the same colour. */
export function hueFor(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0;
  return h % 360;
}

export const errorText = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
