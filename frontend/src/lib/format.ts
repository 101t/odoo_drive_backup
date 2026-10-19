import type { Run } from "../api/types";
import { getLocale, intlLocale, t, type MessageKey } from "../i18n";

const BYTE_UNITS: MessageKey[] = ["unit.b", "unit.kb", "unit.mb", "unit.gb", "unit.tb", "unit.pb"];

export function formatNumber(value: number): string {
  return value.toLocaleString(intlLocale());
}

export function formatBytes(bytes: number | null | undefined, digits = 1): string {
  if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const amount = unit === 0 ? String(bytes) : value.toFixed(digits);
  return t("format.size", { value: amount, unit: t(BYTE_UNITS[unit]) });
}

export function formatRate(bytesPerSecond: number): string {
  return t("format.rate", { size: formatBytes(bytesPerSecond) });
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || ms < 0) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return t("duration.s", { s });
  const m = Math.floor(s / 60);
  if (m < 60) return t("duration.ms", { m, s: String(s % 60).padStart(2, "0") });
  const h = Math.floor(m / 60);
  if (h < 48) return t("duration.hm", { h, m: String(m % 60).padStart(2, "0") });
  return t("duration.dh", { d: Math.floor(h / 24), h: h % 24 });
}

export function timeAgo(iso: string | null | undefined, now: number = Date.now()): string {
  if (!iso) return t("time.never");
  const diff = new Date(iso).getTime() - now;
  const abs = Math.abs(diff);
  if (abs < 45_000) return diff > 0 ? t("time.in_a_moment") : t("time.just_now");
  const relative = new Intl.RelativeTimeFormat(intlLocale(), {
    numeric: "auto",
    style: getLocale() === "ar" ? "long" : "short",
  });
  const sign = diff > 0 ? 1 : -1;
  if (abs < 3_600_000) return relative.format(sign * Math.round(abs / 60_000), "minute");
  if (abs < 86_400_000) return relative.format(sign * Math.round(abs / 3_600_000), "hour");
  if (abs < 30 * 86_400_000) return relative.format(sign * Math.round(abs / 86_400_000), "day");
  return new Date(iso).toLocaleDateString(intlLocale());
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleString(intlLocale(), {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(intlLocale(), {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  });
}

/** Formats an instant in an explicit IANA time zone (e.g. the zone a job is scheduled in). */
export function formatInZone(iso: string, timeZone: string): string {
  try {
    return new Date(iso).toLocaleString(intlLocale(), {
      timeZone,
      weekday: "short",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return formatDateTime(iso);
  }
}

/** Short day label for charts ("Oct 19"). `day` is a UTC date (YYYY-MM-DD). */
export function formatDay(day: string, options: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" }): string {
  return new Date(`${day}T00:00:00Z`).toLocaleDateString(intlLocale(), { ...options, timeZone: "UTC" });
}

export function runDurationMs(run: Pick<Run, "started_at" | "finished_at">, now: number = Date.now()): number | null {
  if (!run.started_at) return null;
  // `now` comes from a periodic tick and may be slightly older than data fetched since.
  const end = run.finished_at ? new Date(run.finished_at).getTime() : Math.max(now, Date.now());
  return Math.max(0, end - new Date(run.started_at).getTime());
}

const PHASES: Record<string, MessageKey> = {
  downloading: "phase.downloading",
  verifying: "phase.verifying",
  uploading: "phase.uploading",
  retention: "phase.retention",
  fetching: "phase.fetching",
  restoring: "phase.restoring",
};

export function phaseLabel(phase: string | null | undefined): string {
  if (!phase) return "";
  const key = PHASES[phase];
  return key ? t(key) : phase;
}

export function defaultTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

export function timezones(): string[] {
  try {
    const intl = Intl as unknown as { supportedValuesOf?: (key: string) => string[] };
    const list = intl.supportedValuesOf?.("timeZone") ?? [];
    return list.includes("UTC") ? list : ["UTC", ...list];
  } catch {
    return ["UTC"];
  }
}
