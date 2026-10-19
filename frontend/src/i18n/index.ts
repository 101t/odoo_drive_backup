import { Fragment, createElement, useSyncExternalStore, type ReactNode } from "react";
import { ar } from "./ar";
import { en, type MessageKey } from "./en";
import { translateServerText } from "./server";
import type { Message } from "./types";

export type { MessageKey } from "./en";
export type { Message, Plural } from "./types";

export type Locale = "en" | "ar";
export type Catalog = Record<MessageKey, Message>;
export type Vars = Record<string, string | number>;

const catalogs: Record<Locale, Catalog> = { en, ar };

export const LANGUAGES: { code: Locale; name: string }[] = [
  { code: "en", name: "English" },
  { code: "ar", name: "العربية" },
];

const STORAGE_KEY = "odb-lang";

function detect(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "ar") return saved;
  } catch {
    // storage unavailable
  }
  const preferred = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  return preferred === "ar" || preferred.startsWith("ar-") ? "ar" : "en";
}

let current: Locale = detect();
const listeners = new Set<() => void>();

function applyToDocument(locale: Locale) {
  const root = document.documentElement;
  root.lang = locale;
  root.dir = locale === "ar" ? "rtl" : "ltr";
}
applyToDocument(current);

export function getLocale(): Locale {
  return current;
}

export function setLocale(locale: Locale) {
  if (locale === current) return;
  current = locale;
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // keep the choice for this session only
  }
  applyToDocument(locale);
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The current locale; re-renders the component when it changes. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, getLocale, getLocale);
}

/** BCP 47 tag for `Intl` formatting. Arabic keeps Western digits, as Odoo does. */
export function intlLocale(locale: Locale = current): string {
  return locale === "ar" ? "ar-u-nu-latn" : "en";
}

export function isRtl(locale: Locale = current): boolean {
  return locale === "ar";
}

const pluralRules = new Map<Locale, Intl.PluralRules>();

function select(message: Message, vars?: Vars): string {
  if (typeof message === "string") return message;
  let rules = pluralRules.get(current);
  if (!rules) {
    rules = new Intl.PluralRules(intlLocale());
    pluralRules.set(current, rules);
  }
  return message[rules.select(Number(vars?.count ?? 0))] ?? message.other;
}

const LATIN = /[A-Za-z/@\\]/;

/**
 * In right-to-left text, wraps values with Latin text (names, paths, e-mails) in a Unicode
 * first-strong isolate so their punctuation stays with them instead of jumping around.
 */
export function isolate(value: string | number): string {
  return current === "ar" && typeof value === "string" && LATIN.test(value) ? `\u2068${value}\u2069` : String(value);
}

/** Translates `key` in the current locale, replacing `{name}` placeholders with `vars`. */
export function t(key: MessageKey, vars?: Vars): string {
  const text = select(catalogs[current][key] ?? en[key], vars);
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in vars ? isolate(vars[name]) : match));
}

/** Like `t`, but placeholders may be React nodes (links, code, emphasis). */
export function tr(key: MessageKey, vars: Record<string, ReactNode>): ReactNode {
  const counts: Vars = typeof vars.count === "number" ? { count: vars.count } : {};
  const parts = select(catalogs[current][key] ?? en[key], counts).split(/(\{\w+\})/);
  return createElement(
    Fragment,
    null,
    ...parts.map((part, index) => {
      const name = /^\{(\w+)\}$/.exec(part)?.[1];
      const raw = name !== undefined && name in vars ? vars[name] : part;
      const value = typeof raw === "string" && name !== undefined ? isolate(raw) : raw;
      return createElement(Fragment, { key: index }, value);
    }),
  );
}

/** Text produced by the server (errors, run logs, test reports) in the current locale. */
export function ts(text: string | null | undefined): string {
  if (!text) return text ?? "";
  return current === "en" ? text : translateServerText(text);
}

/** Subscribes the component to locale changes and returns the translation helpers. */
export function useT() {
  useLocale();
  return t;
}
