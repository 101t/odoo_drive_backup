import { CalendarClock, Plus } from "lucide-react";
import { useDeferredValue, useEffect, useMemo, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useCronPreview, useDestinations, useInstances, useSaveJob } from "../api/hooks";
import type { BackupFormat, Job, JobInput } from "../api/types";
import { DestinationIcon } from "../components/RunBits";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Checkbox, Field, Input, Modal, Segmented, Select, Switch } from "../components/ui";
import { t, ts, useT, type MessageKey } from "../i18n";
import { defaultTimezone, formatDateTime, formatInZone, timezones } from "../lib/format";

const PRESETS: { label: MessageKey; cron: string }[] = [
  { label: "preset.hourly", cron: "0 * * * *" },
  { label: "preset.every_6_hours", cron: "0 */6 * * *" },
  { label: "preset.daily", cron: "0 2 * * *" },
  { label: "preset.twice_daily", cron: "0 2,14 * * *" },
  { label: "preset.weekly", cron: "0 3 * * 0" },
  { label: "preset.monthly", cron: "0 4 1 * *" },
];

const RETENTION_FIELDS = [
  { key: "keep_last", label: "retention.last", hint: "retention.last_hint" },
  { key: "keep_daily", label: "retention.daily", hint: "retention.daily_hint" },
  { key: "keep_weekly", label: "retention.weekly", hint: "retention.weekly_hint" },
  { key: "keep_monthly", label: "retention.monthly", hint: "retention.monthly_hint" },
  { key: "keep_yearly", label: "retention.yearly", hint: "retention.yearly_hint" },
] as const satisfies readonly { key: string; label: MessageKey; hint: MessageKey }[];

type RetentionKey = (typeof RETENTION_FIELDS)[number]["key"];

function emptyJob(): JobInput {
  return {
    name: "",
    instance_id: 0,
    all_databases: false,
    databases: [],
    format: "zip",
    cron: "0 2 * * *",
    timezone: defaultTimezone(),
    enabled: true,
    keep_last: 7,
    keep_daily: 7,
    keep_weekly: 4,
    keep_monthly: 6,
    keep_yearly: 0,
    retries: 2,
    verify_archive: true,
    destination_ids: [],
  };
}

export function jobToInput(job: Job): JobInput {
  return {
    name: job.name,
    instance_id: job.instance_id,
    all_databases: job.all_databases,
    databases: job.databases,
    format: job.format,
    cron: job.cron,
    timezone: job.timezone,
    enabled: job.enabled,
    keep_last: job.keep_last,
    keep_daily: job.keep_daily,
    keep_weekly: job.keep_weekly,
    keep_monthly: job.keep_monthly,
    keep_yearly: job.keep_yearly,
    retries: job.retries,
    verify_archive: job.verify_archive,
    destination_ids: job.destination_ids,
  };
}

export function retentionSummary(job: Pick<JobInput, RetentionKey>): string {
  const parts: string[] = [];
  if (job.keep_last) parts.push(t("retention.summary_last", { count: job.keep_last }));
  if (job.keep_daily) parts.push(t("retention.summary_daily", { count: job.keep_daily }));
  if (job.keep_weekly) parts.push(t("retention.summary_weekly", { count: job.keep_weekly }));
  if (job.keep_monthly) parts.push(t("retention.summary_monthly", { count: job.keep_monthly }));
  if (job.keep_yearly) parts.push(t("retention.summary_yearly", { count: job.keep_yearly }));
  return parts.length ? parts.join(" · ") : t("retention.keep_everything");
}

export function JobForm({ job, open, onClose }: { job: Job | null; open: boolean; onClose: () => void }) {
  const t = useT();
  const { data: instances = [] } = useInstances();
  const { data: destinations = [] } = useDestinations();
  const save = useSaveJob();
  const toast = useToast();
  const [form, setForm] = useState<JobInput>(emptyJob);
  const [extraDb, setExtraDb] = useState("");
  const tzList = useMemo(() => timezones(), []);

  useEffect(() => {
    if (!open) return;
    save.reset();
    setExtraDb("");
    if (job) {
      setForm(jobToInput(job));
    } else {
      const base = emptyJob();
      base.instance_id = instances[0]?.id ?? 0;
      base.destination_ids = destinations.length === 1 ? [destinations[0].id] : [];
      setForm(base);
    }
  }, [open, job?.id]);

  const update = <K extends keyof JobInput>(key: K, value: JobInput[K]) => setForm((f) => ({ ...f, [key]: value }));
  const instance = instances.find((i) => i.id === form.instance_id);
  const deferredCron = useDeferredValue(form.cron);
  const deferredTz = useDeferredValue(form.timezone);
  const preview = useCronPreview(deferredCron, deferredTz);
  const knownDatabases = Array.from(new Set([...(instance?.databases ?? []), ...form.databases])).sort();

  const toggle = <T,>(list: T[], value: T) => (list.includes(value) ? list.filter((v) => v !== value) : [...list, value]);
  const addExtraDatabase = () => {
    if (extraDb.trim()) update("databases", Array.from(new Set([...form.databases, extraDb.trim()])));
    setExtraDb("");
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      const saved = await save.mutateAsync({ id: job?.id, input: form });
      toast.success(
        job ? t("job_form.updated") : t("job_form.created"),
        saved.next_run_at ? t("job_form.next_run", { when: formatDateTime(saved.next_run_at) }) : undefined,
      );
      onClose();
    } catch {
      // shown inline
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      dismissable={false}
      size="xl"
      title={job ? t("job_form.edit_title", { name: job.name }) : t("job_form.new_title")}
      description={t("job_form.description")}
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button type="submit" form="job-form" variant="primary" loading={save.isPending}>
            {job ? t("common.save") : t("job_form.create")}
          </Button>
        </>
      }
    >
      <form id="job-form" onSubmit={submit} className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        {save.error && (
          <div className="lg:col-span-2">
            <Alert tone="red">{errorMessage(save.error)}</Alert>
          </div>
        )}

        <div className="space-y-5">
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field label={t("common.name")}>
              <Input
                value={form.name}
                onChange={(e) => update("name", e.target.value)}
                placeholder={t("job_form.name_placeholder")}
                dir="auto"
                required
              />
            </Field>
            <Field label={t("job_form.instance")}>
              <Select
                value={form.instance_id}
                onChange={(e) => {
                  update("instance_id", Number(e.target.value));
                  update("databases", []);
                }}
                required
              >
                {instances.length === 0 && <option value={0}>{t("job_form.no_instance")}</option>}
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div className="space-y-3">
            <p className="text-sm font-medium">{t("common.databases")}</p>
            <Switch
              checked={form.all_databases}
              onChange={(v) => update("all_databases", v)}
              label={t("jobs.all_databases")}
              description={t("job_form.all_databases_hint")}
            />
            {!form.all_databases && (
              <div className="space-y-3 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                {knownDatabases.length === 0 ? (
                  <p className="text-sm text-zinc-500">{t("job_form.no_databases")}</p>
                ) : (
                  <div className="grid max-h-48 grid-cols-1 gap-2 overflow-y-auto sm:grid-cols-2">
                    {knownDatabases.map((db) => (
                      <Checkbox
                        key={db}
                        checked={form.databases.includes(db)}
                        onChange={() => update("databases", toggle(form.databases, db))}
                        label={<bdi>{db}</bdi>}
                      />
                    ))}
                  </div>
                )}
                <div className="flex gap-2">
                  <Input
                    value={extraDb}
                    onChange={(e) => setExtraDb(e.target.value)}
                    placeholder={t("job_form.other_database")}
                    className="h-8"
                    dir="auto"
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        addExtraDatabase();
                      }
                    }}
                  />
                  <Button size="sm" icon={<Plus />} onClick={addExtraDatabase}>
                    {t("common.add")}
                  </Button>
                </div>
              </div>
            )}
          </div>

          <Field label={t("common.format")}>
            <div>
              <Segmented<BackupFormat>
                value={form.format}
                onChange={(v) => update("format", v)}
                options={[
                  { value: "zip", label: t("format.zip") },
                  { value: "dump", label: t("format.dump") },
                ]}
              />
            </div>
          </Field>

          <div className="space-y-2">
            <p className="text-sm font-medium">{t("common.destinations")}</p>
            {destinations.length === 0 ? (
              <p className="text-sm text-zinc-500">{t("job_form.no_destinations")}</p>
            ) : (
              <div className="space-y-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                {destinations.map((d) => (
                  <Checkbox
                    key={d.id}
                    checked={form.destination_ids.includes(d.id)}
                    onChange={() => update("destination_ids", toggle(form.destination_ids, d.id))}
                    label={
                      <span className="inline-flex items-center gap-1.5">
                        <DestinationIcon kind={d.kind} className="text-zinc-500" /> <bdi>{d.name}</bdi>
                      </span>
                    }
                  />
                ))}
              </div>
            )}
            <p className="text-xs text-zinc-500">{t("job_form.destinations_hint")}</p>
          </div>
        </div>

        <div className="space-y-5">
          <div className="space-y-3">
            <p className="text-sm font-medium">{t("job_form.schedule")}</p>
            <div className="flex flex-wrap gap-1.5">
              {PRESETS.map((preset) => (
                <button
                  key={preset.cron}
                  type="button"
                  onClick={() => update("cron", preset.cron)}
                  className={
                    form.cron === preset.cron
                      ? "rounded-md bg-brand-600 px-2.5 py-1 text-xs font-medium text-white"
                      : "rounded-md bg-zinc-100 px-2.5 py-1 text-xs font-medium text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
                  }
                >
                  {t(preset.label)}
                </button>
              ))}
            </div>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field label={t("job_form.cron")} hint={t("job_form.cron_hint")}>
                <Input
                  value={form.cron}
                  onChange={(e) => update("cron", e.target.value)}
                  className="font-mono"
                  dir="ltr"
                  required
                />
              </Field>
              <Field label={t("job_form.timezone")}>
                <Select value={form.timezone} onChange={(e) => update("timezone", e.target.value)} dir="ltr">
                  {(tzList.includes(form.timezone) ? tzList : [form.timezone, ...tzList]).map((tz) => (
                    <option key={tz} value={tz}>
                      {tz}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>
            <div className="rounded-lg bg-zinc-50 p-3 text-sm dark:bg-zinc-950/50">
              {preview.data?.valid ? (
                <>
                  <p className="flex items-center gap-2 font-medium">
                    <CalendarClock className="size-4 shrink-0 text-brand-600" /> {preview.data.description}
                  </p>
                  <ul className="mt-2 space-y-0.5 text-xs text-zinc-500">
                    {preview.data.next.map((n) => (
                      <li key={n}>
                        {formatInZone(n, form.timezone)}
                        <span className="ms-1.5 opacity-70">{t("job_form.your_time", { when: formatDateTime(n) })}</span>
                      </li>
                    ))}
                  </ul>
                </>
              ) : (
                <p className="text-rose-600 dark:text-rose-400">
                  {preview.data?.error ? ts(preview.data.error) : t("job_form.enter_cron")}
                </p>
              )}
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-baseline justify-between gap-2">
              <p className="text-sm font-medium">{t("job_form.retention")}</p>
              <Badge tone="violet">{retentionSummary(form)}</Badge>
            </div>
            <div className="grid grid-cols-5 gap-2">
              {RETENTION_FIELDS.map((field) => (
                <label key={field.key} className="space-y-1 text-center">
                  <span className="block text-xs font-medium text-zinc-600 dark:text-zinc-400">{t(field.label)}</span>
                  <Input
                    type="number"
                    min={0}
                    max={1000}
                    value={form[field.key]}
                    onChange={(e) => update(field.key, Math.max(0, Number(e.target.value)))}
                    className="text-center"
                  />
                  <span className="block text-[11px] text-zinc-500">{t(field.hint)}</span>
                </label>
              ))}
            </div>
            <p className="text-xs text-zinc-500">{t("job_form.retention_hint")}</p>
          </div>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field label={t("job_form.retries")} hint={t("job_form.retries_hint")}>
              <Input
                type="number"
                min={0}
                max={10}
                value={form.retries}
                onChange={(e) => update("retries", Number(e.target.value))}
              />
            </Field>
          </div>
          <div className="space-y-3">
            <Switch
              checked={form.verify_archive}
              onChange={(v) => update("verify_archive", v)}
              label={t("job_form.verify")}
              description={t("job_form.verify_hint")}
            />
            <Switch
              checked={form.enabled}
              onChange={(v) => update("enabled", v)}
              label={t("job_form.enabled")}
              description={t("job_form.enabled_hint")}
            />
          </div>
        </div>
      </form>
    </Modal>
  );
}
