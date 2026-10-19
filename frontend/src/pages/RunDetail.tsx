import { clsx } from "clsx";
import { ArrowLeft, CircleX, Download, ExternalLink, RotateCcw, Square, Trash } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { errorMessage } from "../api/client";
import { useCancelRun, useDeleteArtifact, useDeleteRun, useRun } from "../api/hooks";
import type { Artifact, RunLog } from "../api/types";
import { useConfirm } from "../components/confirm";
import { RestoreDialog } from "../components/RestoreDialog";
import { DestinationIcon, RunProgress, StatusBadge } from "../components/RunBits";
import { useToast } from "../components/toast";
import {
  Alert,
  Badge,
  Button,
  Card,
  CopyButton,
  EmptyState,
  IconButton,
  IconLink,
  KeyValue,
  LoadingBlock,
  Mono,
  PageHeader,
} from "../components/ui";
import { formatBytes, formatDateTime, formatDuration, runDurationMs } from "../lib/format";
import { useNow } from "../lib/live";

function LogView({ logs }: { logs: RunLog[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [logs.length]);

  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
      className="scrollbar-thin max-h-[28rem] overflow-y-auto rounded-b-xl bg-zinc-950 p-4 font-mono text-xs leading-relaxed text-zinc-300"
    >
      {logs.length === 0 ? (
        <p className="text-zinc-500">No log yet.</p>
      ) : (
        logs.map((log) => (
          <div key={log.id} className="flex gap-3">
            <span className="shrink-0 text-zinc-500">{new Date(log.ts).toLocaleTimeString()}</span>
            <span
              className={clsx(
                "min-w-0 break-words",
                log.level === "error" && "text-rose-400",
                log.level === "warn" && "text-amber-300",
              )}
            >
              {log.message}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

const artifactTone = {
  stored: "green",
  failed: "red",
  skipped: "gray",
  deleted: "gray",
  uploading: "blue",
  pending: "gray",
} as const;

export function RunDetailPage() {
  const id = Number(useParams().id);
  const { data, isLoading, error } = useRun(id);
  const cancel = useCancelRun();
  const deleteRun = useDeleteRun();
  const deleteArtifact = useDeleteArtifact();
  const confirm = useConfirm();
  const toast = useToast();
  const navigate = useNavigate();
  const now = useNow(1000);
  const [restoreFrom, setRestoreFrom] = useState<Artifact | null>(null);

  if (isLoading) return <LoadingBlock />;
  if (error || !data) {
    return <EmptyState icon={<CircleX />} title="Run not found" action={<Link to="/runs">Back to history</Link>} />;
  }
  const { run, logs, restore_options: restore } = data;
  const active = run.status === "queued" || run.status === "running";

  const onCancel = async () => {
    const { confirmed } = await confirm({
      title: "Cancel this run?",
      message: "The transfer stops and temporary files are removed. Partial uploads are discarded.",
      confirmLabel: "Cancel run",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await cancel.mutateAsync(run.id);
    } catch (e) {
      toast.error("Cannot cancel", errorMessage(e));
    }
  };

  const onDelete = async () => {
    const stored = run.artifacts.filter((a) => a.status === "stored").length;
    const { confirmed, checked } = await confirm({
      title: `Delete run #${run.id}?`,
      message: "The run and its logs are removed from the history.",
      checkbox: stored > 0 ? `Also delete the ${stored} stored backup file(s) from their destinations` : undefined,
      checkboxDefault: false,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteRun.mutateAsync({ id: run.id, deleteFiles: checked });
      toast.success(`Run #${run.id} deleted`);
      navigate("/runs");
    } catch (e) {
      toast.error("Cannot delete", errorMessage(e));
    }
  };

  const onDeleteArtifact = async (artifact: Artifact) => {
    const { confirmed } = await confirm({
      title: "Delete this backup file?",
      message: `The file is permanently removed from ${artifact.destination_name}.`,
      confirmLabel: "Delete file",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteArtifact.mutateAsync(artifact.id);
      toast.success("File deleted");
    } catch (e) {
      toast.error("Cannot delete the file", errorMessage(e));
    }
  };

  const summary: [ReactNode, ReactNode][] = [
    ["Instance", run.instance_id ? <Link to={`/instances/${run.instance_id}`} className="hover:underline">{run.instance_name}</Link> : run.instance_name],
    ["Database", run.database],
    [
      run.kind === "restore" ? "Source" : "Job",
      run.kind === "restore" && restore ? (
        <Link to={`/runs/${restore.source_run_id}`} className="hover:underline">
          {restore.source_instance} / {restore.source_database} (run #{restore.source_run_id})
        </Link>
      ) : run.job_id ? (
        <Link to={`/runs?job_id=${run.job_id}`} className="hover:underline">
          {run.job_name}
        </Link>
      ) : (
        "Manual backup"
      ),
    ],
    ["Trigger", run.trigger === "schedule" ? "Scheduled" : "Manual"],
    ["Format", run.format === "zip" ? "ZIP (dump + filestore)" : "PostgreSQL dump"],
    ["Odoo version", run.odoo_version ?? "—"],
    ["Created", formatDateTime(run.created_at)],
    ["Duration", formatDuration(runDurationMs(run, now))],
    ["Size", formatBytes(run.size_bytes)],
    ["Attempts", run.attempt || "—"],
  ];
  if (restore) {
    summary.push([
      "Options",
      <span className="flex flex-wrap gap-1">
        <Badge tone={restore.copy ? "violet" : "amber"}>{restore.copy ? "copy (new UUID)" : "same UUID"}</Badge>
        {restore.neutralize && <Badge tone="blue">neutralized</Badge>}
      </span>,
    ]);
  }
  if (run.file_name) summary.push(["File", <Mono>{run.file_name}</Mono>]);
  if (run.sha256)
    summary.push([
      "SHA-256",
      <span className="flex items-center gap-1">
        <Mono className="truncate">{run.sha256}</Mono>
        <CopyButton value={run.sha256} />
      </span>,
    ]);
  if (run.md5) summary.push(["MD5", <Mono>{run.md5}</Mono>]);

  return (
    <>
      <PageHeader
        back={
          <Link to="/runs" className="mb-2 inline-flex items-center gap-1 text-sm text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100">
            <ArrowLeft className="size-4" /> History
          </Link>
        }
        title={
          <span className="flex items-center gap-3">
            {run.kind === "restore" ? "Restore" : "Backup"} #{run.id}
            <StatusBadge status={run.status} />
          </span>
        }
        description={`${run.instance_name} / ${run.database}`}
        actions={
          active ? (
            <Button variant="danger" icon={<Square />} onClick={onCancel} loading={cancel.isPending}>
              Cancel
            </Button>
          ) : (
            <Button icon={<Trash />} onClick={onDelete}>
              Delete
            </Button>
          )
        }
      />

      <div className="space-y-6">
        {run.error && run.status !== "cancelled" && (
          <Alert tone={run.status === "partial" ? "amber" : "red"} icon={<CircleX />} title={run.status === "partial" ? "Completed with problems" : "Failed"}>
            {run.error}
          </Alert>
        )}

        {active && (
          <Card title="Progress">
            <RunProgress run={run} />
          </Card>
        )}

        <div className="grid gap-6 xl:grid-cols-5">
          <div className="space-y-6 xl:col-span-2">
            <Card title="Summary">
              <KeyValue items={summary} />
            </Card>
          </div>

          <div className="space-y-6 xl:col-span-3">
            {run.kind === "backup" && (
              <Card title="Stored copies" padded={false}>
                {run.artifacts.length === 0 ? (
                  <p className="px-5 py-6 text-center text-sm text-zinc-500">No destination.</p>
                ) : (
                  <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                    {run.artifacts.map((a) => (
                      <li key={a.id} className="flex flex-wrap items-center gap-x-4 gap-y-2 px-5 py-3 sm:flex-nowrap">
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <DestinationIcon kind={a.destination_kind} className="shrink-0 text-zinc-500" />
                            <span className="truncate text-sm font-medium">{a.destination_name}</span>
                          </div>
                          {a.remote_path && (
                            <p className="mt-0.5 truncate text-xs text-zinc-500" title={a.remote_path}>
                              {a.remote_path}
                            </p>
                          )}
                          {a.error && (
                            <p
                              className={clsx(
                                "mt-0.5 text-xs",
                                a.status === "failed" ? "text-rose-600 dark:text-rose-400" : "text-zinc-500",
                              )}
                            >
                              {a.error}
                            </p>
                          )}
                        </div>
                        <Badge tone={artifactTone[a.status]}>
                          {a.status}
                          {a.delete_reason ? ` (${a.delete_reason})` : ""}
                        </Badge>
                        {a.status === "stored" && (
                          <div className="flex shrink-0 gap-0.5">
                            {a.web_url && (
                              <IconLink label="Open in Google Drive" href={a.web_url} external>
                                <ExternalLink />
                              </IconLink>
                            )}
                            <IconLink label="Download" href={`/api/artifacts/${a.id}/download`} download>
                              <Download />
                            </IconLink>
                            <IconButton label="Restore…" onClick={() => setRestoreFrom(a)}>
                              <RotateCcw />
                            </IconButton>
                            <IconButton label="Delete file" tone="danger" onClick={() => onDeleteArtifact(a)}>
                              <Trash />
                            </IconButton>
                          </div>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </Card>
            )}

            <Card title="Log" padded={false}>
              <LogView logs={logs} />
            </Card>
          </div>
        </div>
      </div>

      <RestoreDialog run={run} artifact={restoreFrom} open={restoreFrom !== null} onClose={() => setRestoreFrom(null)} />
    </>
  );
}
