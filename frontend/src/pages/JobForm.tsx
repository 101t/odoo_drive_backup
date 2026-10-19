import { CalendarClock, Plus } from "lucide-react";
import { useDeferredValue, useEffect, useMemo, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useCronPreview, useDestinations, useInstances, useSaveJob } from "../api/hooks";
import type { BackupFormat, Job, JobInput } from "../api/types";
import { DestinationIcon } from "../components/RunBits";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Checkbox, Field, Input, Modal, Segmented, Select, Switch } from "../components/ui";
import { defaultTimezone, formatDateTime, formatInZone, timezones } from "../lib/format";

const PRESETS = [
  { label: "Hourly", cron: "0 * * * *" },
  { label: "Every 6 hours", cron: "0 */6 * * *" },
  { label: "Daily 02:00", cron: "0 2 * * *" },
  { label: "Twice a day", cron: "0 2,14 * * *" },
  { label: "Weekly (Sun 03:00)", cron: "0 3 * * 0" },
  { label: "Monthly (1st 04:00)", cron: "0 4 1 * *" },
];

const RETENTION_FIELDS = [
  { key: "keep_last", label: "Last", hint: "most recent" },
  { key: "keep_daily", label: "Daily", hint: "days" },
  { key: "keep_weekly", label: "Weekly", hint: "weeks" },
  { key: "keep_monthly", label: "Monthly", hint: "months" },
  { key: "keep_yearly", label: "Yearly", hint: "years" },
] as const;

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
  if (job.keep_last) parts.push(`last ${job.keep_last}`);
  if (job.keep_daily) parts.push(`${job.keep_daily} daily`);
  if (job.keep_weekly) parts.push(`${job.keep_weekly} weekly`);
  if (job.keep_monthly) parts.push(`${job.keep_monthly} monthly`);
  if (job.keep_yearly) parts.push(`${job.keep_yearly} yearly`);
  return parts.length ? parts.join(" · ") : "keep everything";
}

export function JobForm({ job, open, onClose }: { job: Job | null; open: boolean; onClose: () => void }) {
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

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      const saved = await save.mutateAsync({ id: job?.id, input: form });
      toast.success(job ? "Job updated" : "Job created", saved.next_run_at ? `Next run: ${formatDateTime(saved.next_run_at)}` : undefined);
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
      title={job ? `Edit ${job.name}` : "New backup job"}
      description="What to back up, when, where to, and how long to keep it."
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form="job-form" variant="primary" loading={save.isPending}>
            {job ? "Save" : "Create job"}
          </Button>
        </>
      }
    >
      <form id="job-form" onSubmit={submit} className="grid gap-6 lg:grid-cols-2">
        {save.error && (
          <div className="lg:col-span-2">
            <Alert tone="red">{errorMessage(save.error)}</Alert>
          </div>
        )}

        <div className="space-y-5">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Name">
              <Input value={form.name} onChange={(e) => update("name", e.target.value)} placeholder="Nightly production" required />
            </Field>
            <Field label="Instance">
              <Select
                value={form.instance_id}
                onChange={(e) => {
                  update("instance_id", Number(e.target.value));
                  update("databases", []);
                }}
                required
              >
                {instances.length === 0 && <option value={0}>No instance: add one first</option>}
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div className="space-y-3">
            <p className="text-sm font-medium">Databases</p>
            <Switch
              checked={form.all_databases}
              onChange={(v) => update("all_databases", v)}
              label="All databases"
              description="Discovered on the server at each run, so new databases are protected automatically."
            />
            {!form.all_databases && (
              <div className="space-y-3 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                {knownDatabases.length === 0 ? (
                  <p className="text-sm text-zinc-500">No database listed for this instance. Add names manually below.</p>
                ) : (
                  <div className="grid max-h-48 grid-cols-1 gap-2 overflow-y-auto sm:grid-cols-2">
                    {knownDatabases.map((db) => (
                      <Checkbox
                        key={db}
                        checked={form.databases.includes(db)}
                        onChange={() => update("databases", toggle(form.databases, db))}
                        label={db}
                      />
                    ))}
                  </div>
                )}
                <div className="flex gap-2">
                  <Input
                    value={extraDb}
                    onChange={(e) => setExtraDb(e.target.value)}
                    placeholder="other database name"
                    className="h-8"
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        if (extraDb.trim()) update("databases", Array.from(new Set([...form.databases, extraDb.trim()])));
                        setExtraDb("");
                      }
                    }}
                  />
                  <Button
                    size="sm"
                    icon={<Plus />}
                    onClick={() => {
                      if (extraDb.trim()) update("databases", Array.from(new Set([...form.databases, extraDb.trim()])));
                      setExtraDb("");
                    }}
                  >
                    Add
                  </Button>
                </div>
              </div>
            )}
          </div>

          <Field label="Format">
            <div>
              <Segmented<BackupFormat>
                value={form.format}
                onChange={(v) => update("format", v)}
                options={[
                  { value: "zip", label: "ZIP with filestore" },
                  { value: "dump", label: "pg_dump only" },
                ]}
              />
            </div>
          </Field>

          <div className="space-y-2">
            <p className="text-sm font-medium">Destinations</p>
            {destinations.length === 0 ? (
              <p className="text-sm text-zinc-500">No destination yet: create one on the Destinations page.</p>
            ) : (
              <div className="space-y-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                {destinations.map((d) => (
                  <Checkbox
                    key={d.id}
                    checked={form.destination_ids.includes(d.id)}
                    onChange={() => update("destination_ids", toggle(form.destination_ids, d.id))}
                    label={
                      <span className="inline-flex items-center gap-1.5">
                        <DestinationIcon kind={d.kind} className="text-zinc-500" /> {d.name}
                      </span>
                    }
                  />
                ))}
              </div>
            )}
            <p className="text-xs text-zinc-500">Each backup is uploaded to every selected destination (3-2-1 rule).</p>
          </div>
        </div>

        <div className="space-y-5">
          <div className="space-y-3">
            <p className="text-sm font-medium">Schedule</p>
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
                  {preset.label}
                </button>
              ))}
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Cron expression" hint="minute hour day month weekday">
                <Input value={form.cron} onChange={(e) => update("cron", e.target.value)} className="font-mono" required />
              </Field>
              <Field label="Time zone">
                <Select value={form.timezone} onChange={(e) => update("timezone", e.target.value)}>
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
                    <CalendarClock className="size-4 text-brand-600" /> {preview.data.description}
                  </p>
                  <ul className="mt-2 space-y-0.5 text-xs text-zinc-500">
                    {preview.data.next.map((n) => (
                      <li key={n}>
                        {formatInZone(n, form.timezone)}
                        <span className="ml-1.5 opacity-70">({formatDateTime(n)} your time)</span>
                      </li>
                    ))}
                  </ul>
                </>
              ) : (
                <p className="text-rose-600 dark:text-rose-400">{preview.data?.error ?? "Enter a cron expression"}</p>
              )}
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-baseline justify-between gap-2">
              <p className="text-sm font-medium">Retention</p>
              <Badge tone="violet">{retentionSummary(form)}</Badge>
            </div>
            <div className="grid grid-cols-5 gap-2">
              {RETENTION_FIELDS.map((field) => (
                <label key={field.key} className="space-y-1 text-center">
                  <span className="block text-xs font-medium text-zinc-600 dark:text-zinc-400">{field.label}</span>
                  <Input
                    type="number"
                    min={0}
                    max={1000}
                    value={form[field.key]}
                    onChange={(e) => update(field.key, Math.max(0, Number(e.target.value)))}
                    className="text-center"
                  />
                  <span className="block text-[11px] text-zinc-500">{field.hint}</span>
                </label>
              ))}
            </div>
            <p className="text-xs text-zinc-500">
              Older backups are deleted from the destinations after each successful run. The newest backup is always
              kept; all zeros keeps everything.
            </p>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Retries" hint="Download attempts after network errors.">
              <Input type="number" min={0} max={10} value={form.retries} onChange={(e) => update("retries", Number(e.target.value))} />
            </Field>
          </div>
          <div className="space-y-3">
            <Switch
              checked={form.verify_archive}
              onChange={(v) => update("verify_archive", v)}
              label="Deep archive verification"
              description="Read every file of the ZIP to validate its CRC before uploading."
            />
            <Switch checked={form.enabled} onChange={(v) => update("enabled", v)} label="Enabled" description="Disabled jobs only run when started manually." />
          </div>
        </div>
      </form>
    </Modal>
  );
}
