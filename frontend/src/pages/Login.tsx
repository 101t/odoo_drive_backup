import { DatabaseBackup, KeyRound } from "lucide-react";
import { useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useLogin } from "../api/hooks";
import { Alert, Button, Field, Input } from "../components/ui";

export function LoginPage({ setup }: { setup: boolean }) {
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
        <div className="mb-8 flex flex-col items-center text-center">
          <div className="mb-4 flex size-12 items-center justify-center rounded-2xl bg-brand-600 text-white shadow-lg shadow-brand-600/30">
            <DatabaseBackup className="size-6" />
          </div>
          <h1 className="text-xl font-semibold tracking-tight">Odoo Backup</h1>
          <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
            {setup ? "Create the administrator account to get started." : "Sign in to manage your backups."}
          </p>
        </div>

        <form
          onSubmit={submit}
          className="space-y-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-xl shadow-zinc-200/50 dark:border-zinc-800 dark:bg-zinc-900 dark:shadow-none"
        >
          {login.error && <Alert tone="red">{errorMessage(login.error)}</Alert>}
          <Field label="Username">
            <Input
              autoFocus={!setup}
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </Field>
          <Field label="Password" hint={setup ? "At least 10 characters." : undefined}>
            <Input
              type="password"
              autoFocus={setup}
              autoComplete={setup ? "new-password" : "current-password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              minLength={setup ? 10 : undefined}
              required
            />
          </Field>
          {setup && (
            <Field label="Confirm password" error={mismatch ? "The passwords do not match" : undefined}>
              <Input
                type="password"
                autoComplete="new-password"
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
            {setup ? "Create account" : "Sign in"}
          </Button>
        </form>
        <p className="mt-6 text-center text-xs text-zinc-500">
          Forgot the password? Run <code className="font-mono">odoo-backup reset-password &lt;user&gt;</code> on the server.
        </p>
      </div>
    </div>
  );
}
