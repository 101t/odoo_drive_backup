import { Bell, Mail, Pencil, Plus, Send, Trash, Webhook } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useChangePassword, useChannels, useDeleteChannel, useSaveSettings, useSettings, useTestChannel } from "../api/hooks";
import type { Channel } from "../api/types";
import { useConfirm } from "../components/confirm";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Card, EmptyState, Field, IconButton, Input, KeyValue, LoadingBlock, Mono, PageHeader } from "../components/ui";
import { formatBytes, formatDateTime } from "../lib/format";
import { NotificationForm } from "./NotificationForm";

const channelIcon = { webhook: <Webhook />, telegram: <Send />, email: <Mail /> };

function Notifications() {
  const { data: channels, isLoading } = useChannels();
  const remove = useDeleteChannel();
  const test = useTestChannel();
  const confirm = useConfirm();
  const toast = useToast();
  const [editing, setEditing] = useState<Channel | null>(null);
  const [open, setOpen] = useState(false);

  const openForm = (channel: Channel | null) => {
    setEditing(channel);
    setOpen(true);
  };

  const runTest = async (channel: Channel) => {
    try {
      const result = await test.mutateAsync(channel.id);
      if (result.ok) toast.success("Test message sent", `Check ${channel.name}.`);
      else toast.error("Test failed", result.error);
    } catch (error) {
      toast.error("Test failed", errorMessage(error));
    }
  };

  const del = async (channel: Channel) => {
    const { confirmed } = await confirm({ title: `Delete ${channel.name}?`, confirmLabel: "Delete", danger: true });
    if (!confirmed) return;
    try {
      await remove.mutateAsync(channel.id);
    } catch (error) {
      toast.error("Cannot delete", errorMessage(error));
    }
  };

  return (
    <Card
      title="Notifications"
      description="Alerts for failed (and optionally successful) runs."
      padded={false}
      actions={
        <Button size="sm" icon={<Plus />} onClick={() => openForm(null)}>
          Add channel
        </Button>
      }
    >
      {isLoading ? (
        <LoadingBlock />
      ) : !channels?.length ? (
        <EmptyState
          icon={<Bell />}
          title="No notification channel"
          description="Get a Slack, Discord, Telegram or e-mail message when a backup fails."
        />
      ) : (
        <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
          {channels.map((channel) => (
            <li key={channel.id} className="flex flex-wrap items-center gap-3 px-5 py-3">
              <div className="flex size-8 items-center justify-center rounded-lg bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300 [&_svg]:size-4">
                {channelIcon[channel.kind]}
              </div>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{channel.name}</p>
                <div className="mt-1 flex flex-wrap gap-1">
                  {!channel.enabled && <Badge tone="gray">disabled</Badge>}
                  {channel.on_failure && <Badge tone="red">failures</Badge>}
                  {channel.on_success && <Badge tone="green">successes</Badge>}
                </div>
              </div>
              <Button size="sm" variant="ghost" onClick={() => runTest(channel)} loading={test.isPending && test.variables === channel.id}>
                Test
              </Button>
              <IconButton label="Edit" onClick={() => openForm(channel)}>
                <Pencil />
              </IconButton>
              <IconButton label="Delete" tone="danger" onClick={() => del(channel)}>
                <Trash />
              </IconButton>
            </li>
          ))}
        </ul>
      )}
      <NotificationForm channel={editing} open={open} onClose={() => setOpen(false)} />
    </Card>
  );
}

function General() {
  const { data } = useSettings();
  const save = useSaveSettings();
  const toast = useToast();
  const [stale, setStale] = useState(26);
  const [logDays, setLogDays] = useState(90);

  useEffect(() => {
    if (data) {
      setStale(data.settings.stale_after_hours);
      setLogDays(data.settings.log_retention_days);
    }
  }, [data]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await save.mutateAsync({ stale_after_hours: stale, log_retention_days: logDays });
      toast.success("Settings saved");
    } catch (error) {
      toast.error("Cannot save", errorMessage(error));
    }
  };

  return (
    <Card title="General">
      <form onSubmit={submit} className="space-y-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Stale after (hours)" hint="Protected databases without a successful backup for this long are flagged.">
            <Input type="number" min={1} max={8760} value={stale} onChange={(e) => setStale(Number(e.target.value))} />
          </Field>
          <Field label="Keep run logs (days)" hint="0 keeps logs forever. Runs themselves are never pruned.">
            <Input type="number" min={0} max={3650} value={logDays} onChange={(e) => setLogDays(Number(e.target.value))} />
          </Field>
        </div>
        <Button type="submit" variant="primary" loading={save.isPending}>
          Save
        </Button>
      </form>
    </Card>
  );
}

function Account() {
  const change = useChangePassword();
  const toast = useToast();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirmNext, setConfirmNext] = useState("");
  const mismatch = confirmNext.length > 0 && next !== confirmNext;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (mismatch) return;
    try {
      await change.mutateAsync({ current_password: current, new_password: next });
      toast.success("Password changed", "Other sessions were signed out.");
      setCurrent("");
      setNext("");
      setConfirmNext("");
    } catch {
      // inline
    }
  };

  return (
    <Card title="Account">
      <form onSubmit={submit} className="space-y-4">
        {change.error && <Alert tone="red">{errorMessage(change.error)}</Alert>}
        <Field label="Current password">
          <Input type="password" value={current} onChange={(e) => setCurrent(e.target.value)} autoComplete="current-password" required />
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="New password" hint="At least 10 characters.">
            <Input type="password" value={next} onChange={(e) => setNext(e.target.value)} minLength={10} autoComplete="new-password" required />
          </Field>
          <Field label="Confirm" error={mismatch ? "The passwords do not match" : undefined}>
            <Input type="password" value={confirmNext} onChange={(e) => setConfirmNext(e.target.value)} autoComplete="new-password" required />
          </Field>
        </div>
        <Button type="submit" variant="primary" loading={change.isPending} disabled={mismatch}>
          Change password
        </Button>
      </form>
    </Card>
  );
}

function System() {
  const { data } = useSettings();
  if (!data) return null;
  const s = data.system;
  return (
    <Card title="System">
      <KeyValue
        items={[
          ["Version", s.version],
          ["Running since", formatDateTime(s.started_at)],
          ["Data directory", <Mono>{s.data_dir}</Mono>],
          [
            "Temp directory",
            <span>
              <Mono>{s.temp_dir}</Mono>
              <span className="ml-2 text-xs text-zinc-500">{formatBytes(s.temp_free_bytes)} free</span>
            </span>,
          ],
          ["Parallel runs", `${s.max_concurrent_jobs} max (ODB_MAX_CONCURRENT_JOBS)`],
          ["Active runs", s.active_runs],
          ["Public URL", s.public_url ?? <span className="text-zinc-500">not set (ODB_PUBLIC_URL)</span>],
          [
            "Prometheus metrics",
            s.metrics_enabled ? (
              <span>
                <Mono>/metrics</Mono> with a bearer token
              </span>
            ) : (
              <span className="text-zinc-500">disabled (set ODB_METRICS_TOKEN)</span>
            ),
          ],
        ]}
      />
      <p className="mt-5 text-xs text-zinc-500">
        Back up <Mono>{s.data_dir}/secret.key</Mono> (or your ODB_SECRET_KEY) somewhere safe: stored passwords and
        credentials cannot be decrypted without it.
      </p>
    </Card>
  );
}

export function SettingsPage() {
  const { isLoading } = useSettings();
  if (isLoading) return <LoadingBlock />;
  return (
    <>
      <PageHeader title="Settings" description="Notifications, preferences and system information." />
      <div className="grid gap-6 xl:grid-cols-2">
        <div className="space-y-6">
          <Notifications />
          <General />
        </div>
        <div className="space-y-6">
          <Account />
          <System />
        </div>
      </div>
    </>
  );
}
