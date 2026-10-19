import { DatabaseBackup, KeyRound } from "lucide-react";
import { useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useLogin } from "../api/hooks";
import { LanguageSwitcher } from "../components/Layout";
import { Alert, Button, Field, Input } from "../components/ui";
import { tr, useT } from "../i18n";

export function LoginPage({ setup }: { setup: boolean }) {
  const t = useT();
  const login = useLogin(setup);
  const [username, setUsername] = useState(setup ? "admin" : "");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const mismatch = setup && confirm.length > 0 && confirm !== password;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (setup && password !== confirm) return;
    login.mutate({ username: username.trim(), password });
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-gradient-to-br from-brand-50 via-zinc-50 to-zinc-100 px-4 dark:from-zinc-950 dark:via-zinc-950 dark:to-brand-950/40">
      <div className="w-full max-w-sm">
        <LanguageSwitcher className="mx-auto mb-8 w-48" />
        <div className="mb-8 flex flex-col items-center text-center">
          <div className="mb-4 flex size-12 items-center justify-center rounded-2xl bg-brand-600 text-white shadow-lg shadow-brand-600/30">
            <DatabaseBackup className="size-6" />
          </div>
          <h1 className="text-xl font-semibold tracking-tight">{t("app.name")}</h1>
          <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
            {setup ? t("login.setup_intro") : t("login.intro")}
          </p>
        </div>

        <form
          onSubmit={submit}
          className="space-y-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-xl shadow-zinc-200/50 dark:border-zinc-800 dark:bg-zinc-900 dark:shadow-none"
        >
          {login.error && <Alert tone="red">{errorMessage(login.error)}</Alert>}
          <Field label={t("login.username")}>
            <Input
              autoFocus={!setup}
              autoComplete="username"
              dir="ltr"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </Field>
          <Field label={t("login.password")} hint={setup ? t("login.password_hint") : undefined}>
            <Input
              type="password"
              autoFocus={setup}
              autoComplete={setup ? "new-password" : "current-password"}
              dir="ltr"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              minLength={setup ? 10 : undefined}
              required
            />
          </Field>
          {setup && (
            <Field label={t("login.confirm_password")} error={mismatch ? t("login.mismatch") : undefined}>
              <Input
                type="password"
                autoComplete="new-password"
                dir="ltr"
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                required
              />
            </Field>
          )}
          <Button
            type="submit"
            variant="primary"
            className="w-full"
            loading={login.isPending}
            icon={<KeyRound />}
            disabled={mismatch}
          >
            {setup ? t("login.create_account") : t("login.sign_in")}
          </Button>
        </form>
        <p className="mt-6 text-center text-xs text-zinc-500">
          {tr("login.forgot", {
            command: (
              <code className="font-mono" dir="ltr">
                odoo-backup reset-password &lt;user&gt;
              </code>
            ),
          })}
        </p>
      </div>
    </div>
  );
}
