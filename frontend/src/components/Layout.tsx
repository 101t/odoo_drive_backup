import { clsx } from "clsx";
import {
  CalendarClock,
  Cloud,
  DatabaseBackup,
  LayoutDashboard,
  ListClock,
  LogOut,
  Menu,
  Monitor,
  Moon,
  Server,
  Settings,
  Sun,
  X,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import { NavLink, useLocation } from "react-router";
import { useAuth, useDashboard, useLogout } from "../api/hooks";
import { LANGUAGES, setLocale, useLocale, useT, type MessageKey } from "../i18n";
import { useTheme, type Theme } from "../lib/theme";

const navigation: { to: string; label: MessageKey; icon: typeof Server; end?: boolean }[] = [
  { to: "/", label: "nav.dashboard", icon: LayoutDashboard, end: true },
  { to: "/instances", label: "nav.instances", icon: Server },
  { to: "/jobs", label: "nav.jobs", icon: CalendarClock },
  { to: "/runs", label: "nav.history", icon: ListClock },
  { to: "/destinations", label: "nav.destinations", icon: Cloud },
  { to: "/settings", label: "nav.settings", icon: Settings },
];

export function LanguageSwitcher({ className }: { className?: string }) {
  const locale = useLocale();
  const t = useT();
  return (
    <div
      role="group"
      aria-label={t("layout.language")}
      className={clsx("flex rounded-lg bg-zinc-100 p-0.5 dark:bg-zinc-800", className)}
    >
      {LANGUAGES.map((language) => (
        <button
          key={language.code}
          type="button"
          lang={language.code}
          aria-pressed={locale === language.code}
          onClick={() => setLocale(language.code)}
          className={clsx(
            "flex flex-1 items-center justify-center rounded-md px-2 py-1 text-xs font-medium",
            locale === language.code
              ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-950 dark:text-zinc-100"
              : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200",
          )}
        >
          {language.name}
        </button>
      ))}
    </div>
  );
}

function ThemeSwitcher() {
  const t = useT();
  const [theme, setTheme] = useTheme();
  const options: { value: Theme; icon: ReactNode; label: string }[] = [
    { value: "light", icon: <Sun />, label: t("theme.light") },
    { value: "system", icon: <Monitor />, label: t("theme.system") },
    { value: "dark", icon: <Moon />, label: t("theme.dark") },
  ];
  return (
    <div className="flex rounded-lg bg-zinc-100 p-0.5 dark:bg-zinc-800">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          title={option.label}
          aria-label={t("theme.label", { theme: option.label })}
          onClick={() => setTheme(option.value)}
          className={clsx(
            "flex flex-1 items-center justify-center rounded-md py-1 [&_svg]:size-3.5",
            theme === option.value
              ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-950 dark:text-zinc-100"
              : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200",
          )}
        >
          {option.icon}
        </button>
      ))}
    </div>
  );
}

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const t = useT();
  const { data: auth } = useAuth();
  const { data: dashboard } = useDashboard();
  const logout = useLogout();
  const active = dashboard?.active.length ?? 0;
  const problems = (dashboard?.counts.failing ?? 0) + (dashboard?.counts.stale ?? 0);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2.5 px-5 py-5">
        <div className="flex size-8 items-center justify-center rounded-lg bg-brand-600 text-white shadow-sm">
          <DatabaseBackup className="size-[18px]" />
        </div>
        <div>
          <p className="text-sm font-semibold leading-tight">{t("app.name")}</p>
          <p className="text-[11px] text-zinc-500" dir="ltr">
            v{auth?.version}
          </p>
        </div>
      </div>

      <nav className="flex-1 space-y-0.5 px-3">
        {navigation.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            onClick={onNavigate}
            className={({ isActive }) =>
              clsx(
                "group flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-brand-50 text-brand-700 dark:bg-brand-500/10 dark:text-brand-300"
                  : "text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800/70 dark:hover:text-zinc-100",
              )
            }
          >
            <item.icon className="size-[18px] shrink-0" />
            <span className="flex-1">{t(item.label)}</span>
            {item.to === "/runs" && active > 0 && (
              <span className="rounded-full bg-sky-100 px-1.5 text-[11px] font-semibold tabular-nums text-sky-700 dark:bg-sky-500/15 dark:text-sky-300">
                {active}
              </span>
            )}
            {item.to === "/" && problems > 0 && (
              <span className="rounded-full bg-rose-100 px-1.5 text-[11px] font-semibold tabular-nums text-rose-700 dark:bg-rose-500/15 dark:text-rose-300">
                {problems}
              </span>
            )}
          </NavLink>
        ))}
      </nav>

      <div className="space-y-3 border-t border-zinc-200 p-4 dark:border-zinc-800">
        <LanguageSwitcher />
        <ThemeSwitcher />
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-zinc-200 text-xs font-semibold uppercase dark:bg-zinc-700">
              {auth?.user?.username.slice(0, 1)}
            </div>
            <span className="truncate text-sm font-medium">{auth?.user?.username}</span>
          </div>
          <button
            type="button"
            onClick={() => logout.mutate()}
            title={t("layout.sign_out")}
            aria-label={t("layout.sign_out")}
            className="rounded-md p-1.5 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-900 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
          >
            <LogOut className="size-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

export function Layout({ children }: { children: ReactNode }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const location = useLocation();

  return (
    <div className="min-h-screen">
      <aside className="fixed inset-y-0 start-0 z-30 hidden w-60 border-e border-zinc-200 bg-white lg:block dark:border-zinc-800 dark:bg-zinc-900">
        <Sidebar />
      </aside>

      <div className="sticky top-0 z-30 flex items-center gap-3 border-b border-zinc-200 bg-white/90 px-4 py-3 backdrop-blur lg:hidden dark:border-zinc-800 dark:bg-zinc-900/90">
        <button
          type="button"
          onClick={() => setOpen(true)}
          aria-label={t("layout.open_menu")}
          className="rounded-md p-1.5 text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800"
        >
          <Menu className="size-5" />
        </button>
        <DatabaseBackup className="size-5 text-brand-600" />
        <span className="text-sm font-semibold">{t("app.name")}</span>
      </div>

      {open && (
        <div className="fixed inset-0 z-40 lg:hidden">
          <div className="absolute inset-0 bg-zinc-950/50" onClick={() => setOpen(false)} aria-hidden />
          <aside className="absolute inset-y-0 start-0 w-64 bg-white shadow-xl dark:bg-zinc-900">
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label={t("layout.close_menu")}
              className="absolute end-3 top-4 rounded-md p-1.5 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
            >
              <X className="size-4" />
            </button>
            <Sidebar onNavigate={() => setOpen(false)} />
          </aside>
        </div>
      )}

      <main className="lg:ps-60">
        <div key={location.pathname} className="mx-auto max-w-7xl px-4 py-6 sm:px-6 lg:px-8 lg:py-8">
          {children}
        </div>
      </main>
    </div>
  );
}
