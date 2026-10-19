import { clsx } from "clsx";
import { ChevronRight, ListClock, RotateCcw, Search } from "lucide-react";
import { useDeferredValue } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useInstances, useRuns } from "../api/hooks";
import { ArtifactChips, RunProgress, StatusBadge } from "../components/RunBits";
import { Button, Card, EmptyState, Input, LoadingBlock, PageHeader, Segmented, Select, tableClass } from "../components/ui";
import { ts, useT } from "../i18n";
import { formatBytes, formatDateTime, formatDuration, runDurationMs, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";

const PAGE_SIZE = 25;

export function RunsPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const { data: instances = [] } = useInstances();
  const now = useNow(5_000);

  const status = params.get("status") ?? "";
  const instanceId = params.get("instance_id") ?? "";
  const jobId = params.get("job_id") ?? "";
  const kind = params.get("kind") ?? "";
  const search = params.get("search") ?? "";
  const page = Math.max(0, Number(params.get("page") ?? 0));
  const deferredSearch = useDeferredValue(search);

  const { data, isLoading, isFetching } = useRuns({
    status: status || undefined,
    instance_id: instanceId ? Number(instanceId) : undefined,
    job_id: jobId ? Number(jobId) : undefined,
    kind: kind || undefined,
    search: deferredSearch || undefined,
    limit: PAGE_SIZE,
    offset: page * PAGE_SIZE,
  });

  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    if (key !== "page") next.delete("page");
    setParams(next, { replace: true });
  };

  const total = data?.total ?? 0;
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));

  return (
    <>
      <PageHeader title={t("runs.title")} description={t("runs.description")} />

      <Card padded={false}>
        <div className="flex flex-col gap-3 border-b border-zinc-100 p-4 lg:flex-row lg:items-center dark:border-zinc-800">
          <Segmented
            value={status || "all"}
            onChange={(v) => set("status", v === "all" ? "" : v)}
            options={[
              { value: "all", label: t("runs.filter_all") },
              { value: "active", label: t("runs.filter_active") },
              { value: "problems", label: t("runs.filter_problems") },
              { value: "success", label: t("runs.filter_success") },
            ]}
          />
          <div className="flex flex-1 flex-col gap-3 sm:flex-row lg:justify-end">
            <Select value={instanceId} onChange={(e) => set("instance_id", e.target.value)} className="sm:w-48">
              <option value="">{t("runs.all_instances")}</option>
              {instances.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}
                </option>
              ))}
            </Select>
            <Select value={kind} onChange={(e) => set("kind", e.target.value)} className="sm:w-36">
              <option value="">{t("runs.all_types")}</option>
              <option value="backup">{t("runs.backups")}</option>
              <option value="restore">{t("runs.restores")}</option>
            </Select>
            <div className="relative sm:w-56">
              <Search className="pointer-events-none absolute start-3 top-1/2 size-4 -translate-y-1/2 text-zinc-400" />
              <Input
                value={search}
                onChange={(e) => set("search", e.target.value)}
                placeholder={t("runs.search")}
                className="ps-9"
              />
            </div>
          </div>
        </div>

        {jobId && (
          <div className="flex items-center justify-between border-b border-zinc-100 bg-brand-50/50 px-4 py-2 text-sm dark:border-zinc-800 dark:bg-brand-500/5">
            <span>{t("runs.filtered_job", { id: jobId })}</span>
            <Button size="sm" variant="ghost" onClick={() => set("job_id", "")}>
              {t("runs.clear")}
            </Button>
          </div>
        )}

        {isLoading ? (
          <LoadingBlock />
        ) : !data?.items.length ? (
          <EmptyState
            icon={<ListClock />}
            title={t("runs.empty")}
            description={status || instanceId || search ? t("runs.empty_filtered") : t("runs.empty_hint")}
          />
        ) : (
          <div className={clsx(tableClass.wrapper, isFetching && "opacity-90")}>
            <table className={tableClass.table}>
              <thead className={tableClass.head}>
                <tr>
                  <th className={tableClass.th}>{t("runs.col_status")}</th>
                  <th className={tableClass.th}>{t("runs.col_run")}</th>
                  <th className={tableClass.th}>{t("runs.col_started")}</th>
                  <th className={clsx(tableClass.th, "text-end")}>{t("runs.col_duration")}</th>
                  <th className={clsx(tableClass.th, "text-end")}>{t("common.size")}</th>
                  <th className={tableClass.th}>{t("runs.col_destinations")}</th>
                  <th className={tableClass.th} />
                </tr>
              </thead>
              <tbody>
                {data.items.map((run) => (
                  <tr
                    key={run.id}
                    className={clsx(tableClass.row, "cursor-pointer hover:bg-zinc-50 dark:hover:bg-zinc-800/40")}
                    onClick={() => navigate(`/runs/${run.id}`)}
                  >
                    <td className={tableClass.td}>
                      <StatusBadge status={run.status} />
                    </td>
                    <td className={clsx(tableClass.td, "max-w-72")}>
                      <Link
                        to={`/runs/${run.id}`}
                        className="block truncate font-medium hover:underline"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {run.kind === "restore" && <RotateCcw className="me-1 inline size-3.5 text-brand-600" />}
                        <bdi>
                          {run.instance_name} / {run.database}
                        </bdi>
                      </Link>
                      <p className="truncate text-xs text-zinc-500">
                        #{run.id} ·{" "}
                        {run.kind === "restore" ? (
                          t("runs.restore")
                        ) : run.job_name ? (
                          <bdi>{run.job_name}</bdi>
                        ) : (
                          t("run.manual")
                        )}
                        {run.trigger === "schedule" ? ` · ${t("runs.scheduled")}` : ""}
                        {run.attempt > 1 ? ` · ${t("runs.attempt", { n: run.attempt })}` : ""}
                      </p>
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap")} title={formatDateTime(run.created_at)}>
                      {timeAgo(run.created_at, now)}
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap text-end tabular-nums")}>
                      {formatDuration(runDurationMs(run, now))}
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap text-end tabular-nums")}>
                      {formatBytes(run.size_bytes)}
                    </td>
                    <td className={tableClass.td}>
                      {run.status === "running" || run.status === "queued" ? (
                        <RunProgress run={run} compact />
                      ) : run.status === "failed" && run.error ? (
                        <p className="line-clamp-1 max-w-sm text-xs text-rose-600 dark:text-rose-400" title={ts(run.error)}>
                          {ts(run.error)}
                        </p>
                      ) : (
                        <ArtifactChips artifacts={run.artifacts} />
                      )}
                    </td>
                    <td className={clsx(tableClass.td, "text-end")}>
                      <ChevronRight className="inline size-4 text-zinc-400 rtl:rotate-180" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {total > PAGE_SIZE && (
          <div className="flex items-center justify-between border-t border-zinc-100 px-4 py-3 text-sm dark:border-zinc-800">
            <span className="text-zinc-500">
              {t("runs.range", {
                from: page * PAGE_SIZE + 1,
                to: Math.min(total, (page + 1) * PAGE_SIZE),
                total,
              })}
            </span>
            <div className="flex gap-2">
              <Button size="sm" disabled={page === 0} onClick={() => set("page", String(page - 1))}>
                {t("runs.previous")}
              </Button>
              <Button size="sm" disabled={page + 1 >= pages} onClick={() => set("page", String(page + 1))}>
                {t("runs.next")}
              </Button>
            </div>
          </div>
        )}
      </Card>
    </>
  );
}
