import { Bell, HeartPulse, Mail, Pencil, Plus, Send, Trash, Webhook } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useChangePassword, useChannels, useDeleteChannel, useSaveSettings, useSettings, useTestChannel } from "../api/hooks";
import type { Channel } from "../api/types";
import { useConfirm } from "../components/confirm";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Card, EmptyState, Field, IconButton, Input, KeyValue, LoadingBlock, Mono, PageHeader } from "../components/ui";
import { tr, ts, useT } from "../i18n";
import { formatBytes, formatDateTime } from "../lib/format";
import { NotificationForm } from "./NotificationForm";

const channelIcon = { webhook: <Webhook />, telegram: <Send />, email: <Mail />, heartbeat: <HeartPulse /> };

function Notifications() {
  const t = useT();
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
      if (result.ok) toast.success(t("notifications.test_sent"), t("notifications.test_sent_hint", { name: channel.name }));
      else toast.error(t("notifications.test_failed"), ts(result.error));
    } catch (error) {
      toast.error(t("notifications.test_failed"), errorMessage(error));
    }
  };

  const del = async (channel: Channel) => {
    const { confirmed } = await confirm({
      title: t("notifications.delete_title", { name: channel.name }),
      confirmLabel: t("common.delete"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await remove.mutateAsync(channel.id);
    } catch (error) {
      toast.error(t("common.cannot_delete"), errorMessage(error));
    }
  };

  return (
    <Card
      title={t("notifications.title")}
      description={t("notifications.description")}
      padded={false}
      actions={
        <Button size="sm" icon={<Plus />} onClick={() => openForm(null)}>
          {t("notifications.add")}
        </Button>
      }
    >
      {isLoading ? (
        <LoadingBlock />
      ) : !channels?.length ? (
        <EmptyState icon={<Bell />} title={t("notifications.empty")} description={t("notifications.empty_hint")} />
      ) : (
        <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
          {channels.map((channel) => (
            <li key={channel.id} className="flex flex-wrap items-center gap-3 px-5 py-3">
              <div className="flex size-8 items-center justify-center rounded-lg bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300 [&_svg]:size-4">
                {channelIcon[channel.kind]}
              </div>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">
                  <bdi>{channel.name}</bdi>
                </p>
                <div className="mt-1 flex flex-wrap gap-1">
                  {!channel.enabled && <Badge tone="gray">{t("jobs.disabled")}</Badge>}
                  {channel.on_failure && <Badge tone="red">{t("notifications.failures")}</Badge>}
                  {channel.on_success && <Badge tone="green">{t("notifications.successes")}</Badge>}
                </div>
              </div>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => runTest(channel)}
                loading={test.isPending && test.variables === channel.id}
              >
                {t("destinations.test")}
              </Button>
              <IconButton label={t("common.edit")} onClick={() => openForm(channel)}>
                <Pencil />
              </IconButton>
              <IconButton label={t("common.delete")} tone="danger" onClick={() => del(channel)}>
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
  const t = useT();
  const { data } = useSettings();
  const save = useSaveSettings();
  const toast = useToast();
  const [stale, setStale] = useState(26);
  const [logDays, setLogDays] = useState(90);
  const [sizeDrop, setSizeDrop] = useState(30);

  useEffect(() => {
    if (data) {
      setStale(data.settings.stale_after_hours);
      setLogDays(data.settings.log_retention_days);
      setSizeDrop(data.settings.size_drop_warn_percent);
    }
  }, [data]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await save.mutateAsync({ stale_after_hours: stale, log_retention_days: logDays, size_drop_warn_percent: sizeDrop });
      toast.success(t("settings.saved"));
    } catch (error) {
      toast.error(t("settings.save_failed"), errorMessage(error));
    }
  };

  return (
    <Card title={t("settings.general")}>
      <form onSubmit={submit} className="space-y-4">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label={t("settings.stale")} hint={t("settings.stale_hint")}>
            <Input type="number" min={1} max={8760} value={stale} onChange={(e) => setStale(Number(e.target.value))} />
          </Field>
          <Field label={t("settings.log_days")} hint={t("settings.log_days_hint")}>
            <Input type="number" min={0} max={3650} value={logDays} onChange={(e) => setLogDays(Number(e.target.value))} />
          </Field>
          <Field label={t("settings.size_drop")} hint={t("settings.size_drop_hint")}>
            <Input type="number" min={0} max={99} value={sizeDrop} onChange={(e) => setSizeDrop(Number(e.target.value))} />
          </Field>
        </div>
        <Button type="submit" variant="primary" loading={save.isPending}>
          {t("common.save")}
        </Button>
      </form>
    </Card>
  );
}

function Account() {
  const t = useT();
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
      toast.success(t("account.changed"), t("account.changed_hint"));
      setCurrent("");
      setNext("");
      setConfirmNext("");
    } catch {
      // inline
    }
  };

  return (
    <Card title={t("account.title")}>
      <form onSubmit={submit} className="space-y-4">
        {change.error && <Alert tone="red">{errorMessage(change.error)}</Alert>}
        <Field label={t("account.current")}>
          <Input
            type="password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            autoComplete="current-password"
            required
          />
        </Field>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label={t("account.new")} hint={t("login.password_hint")}>
            <Input
              type="password"
              value={next}
              onChange={(e) => setNext(e.target.value)}
              minLength={10}
              autoComplete="new-password"
              required
            />
          </Field>
          <Field label={t("common.confirm")} error={mismatch ? t("login.mismatch") : undefined}>
            <Input
              type="password"
              value={confirmNext}
              onChange={(e) => setConfirmNext(e.target.value)}
              autoComplete="new-password"
              required
            />
          </Field>
        </div>
        <Button type="submit" variant="primary" loading={change.isPending} disabled={mismatch}>
          {t("account.change")}
        </Button>
      </form>
    </Card>
  );
}

function System() {
  const t = useT();
  const { data } = useSettings();
  if (!data) return null;
  const s = data.system;
  const database = s.database;
  return (
    <Card title={t("system.title")}>
      <KeyValue
        items={[
          [t("system.version"), <bdi>{s.version}</bdi>],
          [t("system.running_since"), formatDateTime(s.started_at)],
          [
            t("system.database"),
            <span>
              <bdi>
                {database.engine}
                {database.version ? ` ${database.version}` : ""}
              </bdi>
              {database.size_bytes !== null && (
                <span className="ms-2 text-xs text-zinc-500">{formatBytes(database.size_bytes)}</span>
              )}
              <span className="block text-xs text-zinc-500">
                {tr(database.engine === "PostgreSQL" ? "system.database_postgres" : "system.database_sqlite", {
                  var: <Mono>ODB_DATABASE_URL</Mono>,
                })}
              </span>
            </span>,
          ],
          [t("system.data_dir"), <Mono>{s.data_dir}</Mono>],
          [
            t("system.temp_dir"),
            <span>
              <Mono>{s.temp_dir}</Mono>
              <span className="ms-2 text-xs text-zinc-500">
                {t("system.free", { size: formatBytes(s.temp_free_bytes) })}
              </span>
            </span>,
          ],
          [
            t("system.parallel"),
            <span>
              {t("system.parallel_value", { count: s.max_concurrent_jobs })}
              <Mono className="block text-zinc-500 rtl:text-right">ODB_MAX_CONCURRENT_JOBS</Mono>
            </span>,
          ],
          [t("system.active_runs"), s.active_runs],
          [
            t("system.public_url"),
            s.public_url ? (
              <Mono>{s.public_url}</Mono>
            ) : (
              <span className="text-zinc-500">{tr("system.public_url_unset", { var: <Mono>ODB_PUBLIC_URL</Mono> })}</span>
            ),
          ],
          [
            t("system.metrics"),
            s.metrics_enabled ? (
              <span>{tr("system.metrics_enabled", { path: <Mono>/metrics</Mono> })}</span>
            ) : (
              <span className="text-zinc-500">{tr("system.metrics_disabled", { var: <Mono>ODB_METRICS_TOKEN</Mono> })}</span>
            ),
          ],
        ]}
      />
      <p className="mt-5 text-xs text-zinc-500">
        {tr("system.key_warning", {
          path: <Mono>{`${s.data_dir}/secret.key`}</Mono>,
          var: <Mono>ODB_SECRET_KEY</Mono>,
        })}
      </p>
    </Card>
  );
}

export function SettingsPage() {
  const t = useT();
  const { isLoading } = useSettings();
  if (isLoading) return <LoadingBlock />;
  return (
    <>
      <PageHeader title={t("settings.title")} description={t("settings.description")} />
      <div className="grid grid-cols-1 gap-6 xl:grid-cols-2">
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
