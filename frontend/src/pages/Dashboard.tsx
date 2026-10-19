import { clsx } from "clsx";
import {
  Activity,
  CalendarClock,
  CircleCheck,
  Cloud,
  DatabaseBackup,
  HardDrive,
  Play,
  Server,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { useDashboard, useInstances } from "../api/hooks";
import type { Dashboard, Instance } from "../api/types";
import { BackupNowDialog } from "../components/BackupNowDialog";
import { ArtifactChips, DestinationIcon, RunProgress, StatusBadge, StatusDot } from "../components/RunBits";
import { Alert, Badge, Button, Card, EmptyState, IconButton, LoadingBlock, PageHeader, Stat, tableClass } from "../components/ui";
import { formatBytes, formatDateTime, formatDuration, runDurationMs, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";

function Onboarding({ data }: { data: Dashboard }) {
  const steps = [
    {
      done: data.counts.instances > 0,
      title: "Connect an Odoo instance",
      text: "URL and master password of a server running Odoo.",
      to: "/instances",
      icon: <Server />,
    },
    {
      done: data.counts.destinations > 0,
      title: "Add a destination",
      text: "Google Drive (service account or OAuth) or a local/NAS folder.",
      to: "/destinations",
      icon: <Cloud />,
    },
    {
      done: data.counts.jobs > 0,
      title: "Schedule a backup job",
      text: "Pick databases, schedule, destinations and retention.",
      to: "/jobs",
      icon: <CalendarClock />,
    },
  ];
  return (
    <Card title="Get started" description="Three steps to fully automated Odoo backups.">
      <ol className="grid gap-3 md:grid-cols-3">
        {steps.map((step, index) => (
          <li key={step.title}>
            <Link
              to={step.to}
              className={clsx(
                "flex h-full gap-3 rounded-lg border p-4 transition-colors",
                step.done
                  ? "border-emerald-200 bg-emerald-50/60 dark:border-emerald-500/30 dark:bg-emerald-500/5"
                  : "border-zinc-200 hover:border-brand-300 hover:bg-brand-50/40 dark:border-zinc-800 dark:hover:border-brand-500/40",
              )}
            >
              <div
                className={clsx(
                  "flex size-8 shrink-0 items-center justify-center rounded-full text-sm font-semibold [&_svg]:size-4",
                  step.done
                    ? "bg-emerald-500 text-white"
                    : "bg-brand-100 text-brand-700 dark:bg-brand-500/15 dark:text-brand-300",
                )}
              >
                {step.done ? <CircleCheck /> : index + 1}
              </div>
              <div>
                <p className="text-sm font-medium">{step.title}</p>
                <p className="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">{step.text}</p>
              </div>
            </Link>
          </li>
        ))}
      </ol>
    </Card>
  );
}

function ActivityChart({ data }: { data: Dashboard["activity"] }) {
  const max = Math.max(1, ...data.map((d) => d.success + d.failed));
  return (
    <div>
      <div className="flex h-32 items-end gap-1.5">
        {data.map((day) => {
          const total = day.success + day.failed;
          const label = new Date(`${day.day}T12:00:00Z`).toLocaleDateString(undefined, { weekday: "short", day: "numeric" });
          return (
            <div
              key={day.day}
              className="group relative flex h-full flex-1 flex-col justify-end"
              title={`${label}: ${day.success} succeeded, ${day.failed} failed, ${formatBytes(day.bytes)}`}
            >
              {total === 0 ? (
                <div className="h-0.5 rounded-full bg-zinc-200 dark:bg-zinc-800" />
              ) : (
                <div className="flex flex-col overflow-hidden rounded-t-[4px]" style={{ height: `${(total / max) * 100}%` }}>
                  {day.failed > 0 && <div className="bg-rose-400 dark:bg-rose-500" style={{ flexGrow: day.failed }} />}
                  {day.success > 0 && (
                    <div className="bg-emerald-400 group-hover:bg-emerald-500 dark:bg-emerald-500" style={{ flexGrow: day.success }} />
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div className="mt-2 flex justify-between text-[11px] text-zinc-500">
        <span>{data[0] && new Date(`${data[0].day}T12:00:00Z`).toLocaleDateString(undefined, { month: "short", day: "numeric" })}</span>
        <span>Today</span>
      </div>
      <div className="mt-3 flex gap-4 text-xs text-zinc-500">
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2 rounded-sm bg-emerald-400" /> Succeeded
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2 rounded-sm bg-rose-400" /> Failed
        </span>
      </div>
    </div>
  );
}

export function DashboardPage() {
  const { data, isLoading } = useDashboard();
  const { data: instances = [] } = useInstances();
  const now = useNow(30_000);
  const [backup, setBackup] = useState<{ instance: Instance; database: string } | null>(null);

  if (isLoading || !data) return <LoadingBlock />;

  const storedBytes = data.storage.reduce((sum, s) => sum + s.bytes, 0);
  const storedFiles = data.storage.reduce((sum, s) => sum + s.files, 0);
  const configured = data.counts.instances > 0 && data.counts.destinations > 0 && data.counts.jobs > 0;
  const healthy = data.counts.failing === 0 && data.counts.stale === 0;

  return (
    <>
      <PageHeader
        title="Dashboard"
        description="Backup health of every Odoo database you manage."
        actions={
          <Link to="/runs">
            <Button icon={<Activity />}>View history</Button>
          </Link>
        }
      />

      <div className="space-y-6">
        {!configured && <Onboarding data={data} />}

        {data.counts.failing > 0 && (
          <Alert tone="red" icon={<TriangleAlert />} title={`${data.counts.failing} database(s) failed their last backup`}>
            Open the run in History to see the full log, then fix the cause or retry with “Back up now”.
          </Alert>
        )}
        {data.counts.stale > 0 && (
          <Alert
            tone="amber"
            icon={<TriangleAlert />}
            title={`${data.counts.stale} protected database(s) without a successful backup in the last ${data.stale_after_hours} hours`}
          >
            Check that their jobs are enabled and the scheduler is running.
          </Alert>
        )}

        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <Stat
            label="Protected databases"
            value={
              <>
                {data.counts.protected}
                <span className="text-base font-normal text-zinc-400"> / {data.counts.databases}</span>
              </>
            }
            sub={healthy ? "All backups up to date" : `${data.counts.stale} stale · ${data.counts.failing} failing`}
            icon={<ShieldCheck />}
            tone={healthy ? "green" : "amber"}
          />
          <Stat
            label="Last 24 hours"
            value={data.last24h.success}
            sub={`successful · ${data.last24h.failed} failed · ${data.last24h.partial} partial`}
            icon={<DatabaseBackup />}
            tone={data.last24h.failed > 0 ? "red" : "violet"}
          />
          <Stat
            label="Backed up (24h)"
            value={formatBytes(data.last24h.bytes)}
            sub={`${data.counts.jobs_enabled} active job(s)`}
            icon={<Activity />}
            tone="blue"
          />
          <Stat
            label="Stored"
            value={formatBytes(storedBytes)}
            sub={`${storedFiles} file(s) on ${data.counts.destinations} destination(s)`}
            icon={<HardDrive />}
            tone="gray"
          />
        </div>

        <div className="grid grid-cols-1 gap-6 xl:grid-cols-3">
          <div className="space-y-6 xl:col-span-2">
            {data.active.length > 0 && (
              <Card title="In progress" description="Live progress of running backups and restores." padded={false}>
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.active.map((run) => (
                    <li key={run.id} className="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-center">
                      <Link to={`/runs/${run.id}`} className="min-w-0 flex-1 hover:underline">
                        <p className="truncate text-sm font-medium">
                          {run.kind === "restore" ? "Restore → " : ""}
                          {run.instance_name} / {run.database}
                        </p>
                        <p className="text-xs text-zinc-500">
                          #{run.id} · {run.job_name ?? "manual"} · {formatDuration(runDurationMs(run, now))}
                        </p>
                      </Link>
                      <div className="sm:w-72">
                        <RunProgress run={run} />
                      </div>
                    </li>
                  ))}
                </ul>
              </Card>
            )}

            <Card title="Databases" description="Last backup of every known database, problems first." padded={false}>
              {data.health.length === 0 ? (
                <EmptyState
                  icon={<Server />}
                  title="No databases yet"
                  description="Add an Odoo instance: its databases are discovered automatically."
                  action={
                    <Link to="/instances">
                      <Button variant="primary">Add an instance</Button>
                    </Link>
                  }
                />
              ) : (
                <div className={tableClass.wrapper}>
                  <table className={tableClass.table}>
                    <thead className={tableClass.head}>
                      <tr>
                        <th className={tableClass.th}>Database</th>
                        <th className={tableClass.th}>Last success</th>
                        <th className={clsx(tableClass.th, "text-right")}>Size</th>
                        <th className={tableClass.th}>Last run</th>
                        <th className={tableClass.th} />
                      </tr>
                    </thead>
                    <tbody>
                      {data.health.map((h) => {
                        const instance = instances.find((i) => i.id === h.instance_id);
                        return (
                          <tr key={`${h.instance_id}-${h.database}`} className={tableClass.row}>
                            <td className={tableClass.td}>
                              <div className="flex items-center gap-2.5">
                                <StatusDot status={h.last_status} />
                                <div className="min-w-0">
                                  <Link to={`/instances/${h.instance_id}`} className="block truncate font-medium hover:underline">
                                    {h.database}
                                  </Link>
                                  <p className="truncate text-xs text-zinc-500">{h.instance_name}</p>
                                </div>
                              </div>
                            </td>
                            <td className={clsx(tableClass.td, "whitespace-nowrap")}>
                              <span title={formatDateTime(h.last_success_at)}>{timeAgo(h.last_success_at, now)}</span>
                              {h.stale && (
                                <Badge tone="amber" className="ml-2">
                                  stale
                                </Badge>
                              )}
                              {!h.protected && (
                                <Badge tone="gray" className="ml-2">
                                  not scheduled
                                </Badge>
                              )}
                            </td>
                            <td className={clsx(tableClass.td, "whitespace-nowrap text-right tabular-nums")}>
                              {formatBytes(h.last_size)}
                            </td>
                            <td className={tableClass.td}>
                              <StatusBadge status={h.last_status} />
                            </td>
                            <td className={clsx(tableClass.td, "text-right")}>
                              {instance && (
                                <IconButton label="Back up now" onClick={() => setBackup({ instance, database: h.database })}>
                                  <Play />
                                </IconButton>
                              )}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )}
            </Card>

            <Card
              title="Recent runs"
              padded={false}
              actions={
                <Link to="/runs" className="text-xs font-medium text-brand-600 hover:underline dark:text-brand-400">
                  View all
                </Link>
              }
            >
              {data.recent.length === 0 ? (
                <p className="px-5 py-8 text-center text-sm text-zinc-500">No backup has run yet.</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.recent.map((run) => (
                    <li key={run.id}>
                      <Link
                        to={`/runs/${run.id}`}
                        className="flex flex-col gap-2 px-5 py-3 hover:bg-zinc-50 sm:flex-row sm:items-center dark:hover:bg-zinc-800/40"
                      >
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium">
                            {run.kind === "restore" ? "Restore → " : ""}
                            {run.instance_name} / {run.database}
                          </p>
                          <p className="truncate text-xs text-zinc-500">
                            {timeAgo(run.created_at, now)} · {formatDuration(runDurationMs(run, now))} ·{" "}
                            {formatBytes(run.size_bytes)}
                          </p>
                        </div>
                        <ArtifactChips artifacts={run.artifacts} />
                        <StatusBadge status={run.status} />
                      </Link>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          </div>

          <div className="space-y-6">
            <Card title="Last 14 days">
              <ActivityChart data={data.activity} />
            </Card>

            <Card title="Upcoming" padded={false}>
              {data.upcoming.length === 0 ? (
                <p className="px-5 py-6 text-center text-sm text-zinc-500">No scheduled job.</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.upcoming.map((u) => (
                    <li key={u.job_id} className="flex items-center justify-between gap-3 px-5 py-3">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium">{u.job_name}</p>
                        <p className="truncate text-xs text-zinc-500">{u.instance_name}</p>
                      </div>
                      <span className="shrink-0 text-xs text-zinc-500" title={formatDateTime(u.next_run_at)}>
                        {timeAgo(u.next_run_at, now)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </Card>

            <Card title="Storage" padded={false}>
              {data.storage.length === 0 ? (
                <p className="px-5 py-6 text-center text-sm text-zinc-500">No destination configured.</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.storage.map((s) => (
                    <li key={s.id} className="flex items-center gap-3 px-5 py-3">
                      <div className="flex size-8 items-center justify-center rounded-lg bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
                        <DestinationIcon kind={s.kind} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{s.name}</p>
                        <p className="text-xs text-zinc-500">{s.files} file(s)</p>
                      </div>
                      <span className="text-sm tabular-nums">{formatBytes(s.bytes)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          </div>
        </div>
      </div>

      <BackupNowDialog
        open={backup !== null}
        instance={backup?.instance ?? null}
        preselected={backup ? [backup.database] : undefined}
        onClose={() => setBackup(null)}
      />
    </>
  );
}
