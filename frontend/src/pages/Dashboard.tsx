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
import type { Dashboard, Instance, Run } from "../api/types";
import { BackupNowDialog } from "../components/BackupNowDialog";
import { ArtifactChips, DestinationIcon, RunProgress, StatusBadge, StatusDot } from "../components/RunBits";
import { Alert, Badge, Button, Card, EmptyState, IconButton, LoadingBlock, PageHeader, Stat, tableClass } from "../components/ui";
import { t, useT } from "../i18n";
import { formatBytes, formatDateTime, formatDay, formatDuration, runDurationMs, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";

/** "Restore → instance / database" or "instance / database". */
export function runTitle(run: Pick<Run, "kind" | "instance_name" | "database">) {
  const name = (
    <bdi>
      {run.instance_name} / {run.database}
    </bdi>
  );
  return run.kind === "restore" ? (
    <>
      {t("run.restore_prefix")} {name}
    </>
  ) : (
    name
  );
}

function Onboarding({ data }: { data: Dashboard }) {
  const t = useT();
  const steps = [
    {
      done: data.counts.instances > 0,
      title: t("onboarding.instance"),
      text: t("onboarding.instance_hint"),
      to: "/instances",
      icon: <Server />,
    },
    {
      done: data.counts.destinations > 0,
      title: t("onboarding.destination"),
      text: t("onboarding.destination_hint"),
      to: "/destinations",
      icon: <Cloud />,
    },
    {
      done: data.counts.jobs > 0,
      title: t("onboarding.job"),
      text: t("onboarding.job_hint"),
      to: "/jobs",
      icon: <CalendarClock />,
    },
  ];
  return (
    <Card title={t("onboarding.title")} description={t("onboarding.description")}>
      <ol className="grid grid-cols-1 gap-3 md:grid-cols-3">
        {steps.map((step, index) => (
          <li key={step.to}>
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
  const t = useT();
  const max = Math.max(1, ...data.map((d) => d.success + d.failed));
  return (
    <div>
      <div className="flex h-32 items-end gap-1.5">
        {data.map((day) => {
          const total = day.success + day.failed;
          const label = formatDay(day.day, { weekday: "short", day: "numeric" });
          return (
            <div
              key={day.day}
              className="group relative flex h-full flex-1 flex-col justify-end"
              title={t("chart.day_tooltip", {
                day: label,
                success: day.success,
                failed: day.failed,
                size: formatBytes(day.bytes),
              })}
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
        <span>{data[0] && formatDay(data[0].day)}</span>
        <span>{t("chart.today")}</span>
      </div>
      <div className="mt-3 flex gap-4 text-xs text-zinc-500">
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2 rounded-sm bg-emerald-400" /> {t("chart.succeeded")}
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2 rounded-sm bg-rose-400" /> {t("chart.failed")}
        </span>
      </div>
    </div>
  );
}

export function DashboardPage() {
  const t = useT();
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
        title={t("dashboard.title")}
        description={t("dashboard.description")}
        actions={
          <Link to="/runs">
            <Button icon={<Activity />}>{t("dashboard.view_history")}</Button>
          </Link>
        }
      />

      <div className="space-y-6">
        {!configured && <Onboarding data={data} />}

        {data.counts.failing > 0 && (
          <Alert tone="red" icon={<TriangleAlert />} title={t("dashboard.failing_title", { count: data.counts.failing })}>
            {t("dashboard.failing_hint")}
          </Alert>
        )}
        {data.counts.stale > 0 && (
          <Alert
            tone="amber"
            icon={<TriangleAlert />}
            title={t("dashboard.stale_title", { count: data.counts.stale, hours: data.stale_after_hours })}
          >
            {t("dashboard.stale_hint")}
          </Alert>
        )}

        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <Stat
            label={t("dashboard.protected")}
            value={
              <>
                {data.counts.protected}
                <span className="text-base font-normal text-zinc-400"> / {data.counts.databases}</span>
              </>
            }
            sub={
              healthy
                ? t("dashboard.all_up_to_date")
                : t("dashboard.stale_failing", { stale: data.counts.stale, failing: data.counts.failing })
            }
            icon={<ShieldCheck />}
            tone={healthy ? "green" : "amber"}
          />
          <Stat
            label={t("dashboard.last_24h")}
            value={data.last24h.success}
            sub={t("dashboard.last_24h_sub", { failed: data.last24h.failed, partial: data.last24h.partial })}
            icon={<DatabaseBackup />}
            tone={data.last24h.failed > 0 ? "red" : "violet"}
          />
          <Stat
            label={t("dashboard.backed_up_24h")}
            value={formatBytes(data.last24h.bytes)}
            sub={t("dashboard.active_jobs", { count: data.counts.jobs_enabled })}
            icon={<Activity />}
            tone="blue"
          />
          <Stat
            label={t("dashboard.stored")}
            value={formatBytes(storedBytes)}
            sub={t("dashboard.stored_sub", {
              files: t("count.files", { count: storedFiles }),
              destinations: t("count.destinations", { count: data.counts.destinations }),
            })}
            icon={<HardDrive />}
            tone="gray"
          />
        </div>

        <div className="grid grid-cols-1 gap-6 xl:grid-cols-3">
          <div className="space-y-6 xl:col-span-2">
            {data.active.length > 0 && (
              <Card title={t("dashboard.in_progress")} description={t("dashboard.in_progress_hint")} padded={false}>
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.active.map((run) => (
                    <li key={run.id} className="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-center">
                      <Link to={`/runs/${run.id}`} className="min-w-0 flex-1 hover:underline">
                        <p className="truncate text-sm font-medium">{runTitle(run)}</p>
                        <p className="text-xs text-zinc-500">
                          #{run.id} · {run.job_name ? <bdi>{run.job_name}</bdi> : t("run.manual")} ·{" "}
                          {formatDuration(runDurationMs(run, now))}
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

            <Card title={t("dashboard.databases")} description={t("dashboard.databases_hint")} padded={false}>
              {data.health.length === 0 ? (
                <EmptyState
                  icon={<Server />}
                  title={t("dashboard.no_databases")}
                  description={t("dashboard.no_databases_hint")}
                  action={
                    <Link to="/instances">
                      <Button variant="primary">{t("dashboard.add_instance")}</Button>
                    </Link>
                  }
                />
              ) : (
                <div className={tableClass.wrapper}>
                  <table className={tableClass.table}>
                    <thead className={tableClass.head}>
                      <tr>
                        <th className={tableClass.th}>{t("common.database")}</th>
                        <th className={tableClass.th}>{t("dashboard.last_success")}</th>
                        <th className={clsx(tableClass.th, "text-end")}>{t("common.size")}</th>
                        <th className={tableClass.th}>{t("dashboard.last_run")}</th>
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
                                    <bdi>{h.database}</bdi>
                                  </Link>
                                  <p className="truncate text-xs text-zinc-500">
                                    <bdi>{h.instance_name}</bdi>
                                  </p>
                                </div>
                              </div>
                            </td>
                            <td className={clsx(tableClass.td, "whitespace-nowrap")}>
                              <span title={formatDateTime(h.last_success_at)}>{timeAgo(h.last_success_at, now)}</span>
                              {h.stale && (
                                <Badge tone="amber" className="ms-2">
                                  {t("dashboard.stale")}
                                </Badge>
                              )}
                              {!h.protected && (
                                <Badge tone="gray" className="ms-2">
                                  {t("dashboard.not_scheduled")}
                                </Badge>
                              )}
                            </td>
                            <td className={clsx(tableClass.td, "whitespace-nowrap text-end tabular-nums")}>
                              {formatBytes(h.last_size)}
                            </td>
                            <td className={tableClass.td}>
                              <StatusBadge status={h.last_status} />
                            </td>
                            <td className={clsx(tableClass.td, "text-end")}>
                              {instance && (
                                <IconButton
                                  label={t("common.backup_now")}
                                  onClick={() => setBackup({ instance, database: h.database })}
                                >
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
              title={t("dashboard.recent_runs")}
              padded={false}
              actions={
                <Link to="/runs" className="text-xs font-medium text-brand-600 hover:underline dark:text-brand-400">
                  {t("dashboard.view_all")}
                </Link>
              }
            >
              {data.recent.length === 0 ? (
                <p className="px-5 py-8 text-center text-sm text-zinc-500">{t("dashboard.no_runs")}</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.recent.map((run) => (
                    <li key={run.id}>
                      <Link
                        to={`/runs/${run.id}`}
                        className="flex flex-col gap-2 px-5 py-3 hover:bg-zinc-50 sm:flex-row sm:items-center dark:hover:bg-zinc-800/40"
                      >
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium">{runTitle(run)}</p>
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
            <Card title={t("dashboard.last_14_days")}>
              <ActivityChart data={data.activity} />
            </Card>

            <Card title={t("dashboard.upcoming")} padded={false}>
              {data.upcoming.length === 0 ? (
                <p className="px-5 py-6 text-center text-sm text-zinc-500">{t("dashboard.no_upcoming")}</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.upcoming.map((u) => (
                    <li key={u.job_id} className="flex items-center justify-between gap-3 px-5 py-3">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium">
                          <bdi>{u.job_name}</bdi>
                        </p>
                        <p className="truncate text-xs text-zinc-500">
                          <bdi>{u.instance_name}</bdi>
                        </p>
                      </div>
                      <span className="shrink-0 text-xs text-zinc-500" title={formatDateTime(u.next_run_at)}>
                        {timeAgo(u.next_run_at, now)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </Card>

            <Card title={t("dashboard.storage")} padded={false}>
              {data.storage.length === 0 ? (
                <p className="px-5 py-6 text-center text-sm text-zinc-500">{t("dashboard.no_storage")}</p>
              ) : (
                <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.storage.map((s) => (
                    <li key={s.id} className="flex items-center gap-3 px-5 py-3">
                      <div className="flex size-8 items-center justify-center rounded-lg bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
                        <DestinationIcon kind={s.kind} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">
                          <bdi>{s.name}</bdi>
                        </p>
                        <p className="text-xs text-zinc-500">{t("count.files", { count: s.files })}</p>
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
