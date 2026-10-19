import { clsx } from "clsx";
import { ChevronRight, ListClock, RotateCcw, Search } from "lucide-react";
import { useDeferredValue } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useInstances, useRuns } from "../api/hooks";
import { ArtifactChips, RunProgress, StatusBadge } from "../components/RunBits";
import { Button, Card, EmptyState, Input, LoadingBlock, PageHeader, Segmented, Select, tableClass } from "../components/ui";
import { formatBytes, formatDateTime, formatDuration, runDurationMs, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";

const PAGE_SIZE = 25;

export function RunsPage() {
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
      <PageHeader title="History" description="Every backup and restore run, with live progress and full logs." />

      <Card padded={false}>
        <div className="flex flex-col gap-3 border-b border-zinc-100 p-4 lg:flex-row lg:items-center dark:border-zinc-800">
          <Segmented
            value={status || "all"}
            onChange={(v) => set("status", v === "all" ? "" : v)}
            options={[
              { value: "all", label: "All" },
              { value: "active", label: "Active" },
              { value: "problems", label: "Problems" },
              { value: "success", label: "Successful" },
            ]}
          />
          <div className="flex flex-1 flex-col gap-3 sm:flex-row lg:justify-end">
            <Select value={instanceId} onChange={(e) => set("instance_id", e.target.value)} className="sm:w-48">
              <option value="">All instances</option>
              {instances.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}
                </option>
              ))}
            </Select>
            <Select value={kind} onChange={(e) => set("kind", e.target.value)} className="sm:w-36">
              <option value="">All types</option>
              <option value="backup">Backups</option>
              <option value="restore">Restores</option>
            </Select>
            <div className="relative sm:w-56">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-zinc-400" />
              <Input value={search} onChange={(e) => set("search", e.target.value)} placeholder="Search database…" className="pl-9" />
            </div>
          </div>
        </div>

        {jobId && (
          <div className="flex items-center justify-between border-b border-zinc-100 bg-brand-50/50 px-4 py-2 text-sm dark:border-zinc-800 dark:bg-brand-500/5">
            <span>Filtered on job #{jobId}</span>
            <Button size="sm" variant="ghost" onClick={() => set("job_id", "")}>
              Clear
            </Button>
          </div>
        )}

        {isLoading ? (
          <LoadingBlock />
        ) : !data?.items.length ? (
          <EmptyState
            icon={<ListClock />}
            title="No run found"
            description={status || instanceId || search ? "Try other filters." : "Runs appear here as soon as a job starts or you click “Back up now”."}
          />
        ) : (
          <div className={clsx(tableClass.wrapper, isFetching && "opacity-90")}>
            <table className={tableClass.table}>
              <thead className={tableClass.head}>
                <tr>
                  <th className={tableClass.th}>Status</th>
                  <th className={tableClass.th}>Run</th>
                  <th className={tableClass.th}>Started</th>
                  <th className={clsx(tableClass.th, "text-right")}>Duration</th>
                  <th className={clsx(tableClass.th, "text-right")}>Size</th>
                  <th className={tableClass.th}>Destinations / progress</th>
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
                      <Link to={`/runs/${run.id}`} className="block truncate font-medium hover:underline" onClick={(e) => e.stopPropagation()}>
                        {run.kind === "restore" && <RotateCcw className="mr-1 inline size-3.5 text-brand-600" />}
                        {run.instance_name} / {run.database}
                      </Link>
                      <p className="truncate text-xs text-zinc-500">
                        #{run.id} · {run.kind === "restore" ? "restore" : run.job_name ?? "manual"}
                        {run.trigger === "schedule" ? " · scheduled" : ""}
                        {run.attempt > 1 ? ` · attempt ${run.attempt}` : ""}
                      </p>
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap")} title={formatDateTime(run.created_at)}>
                      {timeAgo(run.created_at, now)}
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap text-right tabular-nums")}>
                      {formatDuration(runDurationMs(run, now))}
                    </td>
                    <td className={clsx(tableClass.td, "whitespace-nowrap text-right tabular-nums")}>{formatBytes(run.size_bytes)}</td>
                    <td className={tableClass.td}>
                      {run.status === "running" || run.status === "queued" ? (
                        <RunProgress run={run} compact />
                      ) : run.status === "failed" && run.error ? (
                        <p className="line-clamp-1 max-w-sm text-xs text-rose-600 dark:text-rose-400" title={run.error}>
                          {run.error}
                        </p>
                      ) : (
                        <ArtifactChips artifacts={run.artifacts} />
                      )}
                    </td>
                    <td className={clsx(tableClass.td, "text-right")}>
                      <ChevronRight className="inline size-4 text-zinc-400" />
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
              {page * PAGE_SIZE + 1}–{Math.min(total, (page + 1) * PAGE_SIZE)} of {total}
            </span>
            <div className="flex gap-2">
              <Button size="sm" disabled={page === 0} onClick={() => set("page", String(page - 1))}>
                Previous
              </Button>
              <Button size="sm" disabled={page + 1 >= pages} onClick={() => set("page", String(page + 1))}>
                Next
              </Button>
            </div>
          </div>
        )}
      </Card>
    </>
  );
}
