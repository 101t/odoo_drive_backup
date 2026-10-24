import { clsx } from "clsx";
import { ArrowLeft, CircleX, Download, ExternalLink, Pin, PinOff, RotateCcw, Square, Trash, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { errorMessage } from "../api/client";
import { useCancelRun, useDeleteArtifact, useDeleteRun, usePinArtifact, useRun } from "../api/hooks";
import type { Artifact, Manifest, RunLog } from "../api/types";
import { useConfirm } from "../components/confirm";
import { RestoreDialog } from "../components/RestoreDialog";
import { DestinationIcon, RunProgress, StatusBadge, artifactStatusLabel, deleteReasonLabel } from "../components/RunBits";
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
import { ts, useT } from "../i18n";
import { formatBytes, formatDateTime, formatDuration, formatTime, runDurationMs } from "../lib/format";
import { useNow } from "../lib/live";

function LogView({ logs }: { logs: RunLog[] }) {
  const t = useT();
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
        <p className="text-zinc-500">{t("run_detail.no_log")}</p>
      ) : (
        logs.map((log) => (
          <div key={log.id} className="flex gap-3">
            <span className="shrink-0 text-zinc-500" dir="ltr">
              {formatTime(log.ts)}
            </span>
            <span
              dir="auto"
              className={clsx(
                "min-w-0 break-words rtl:font-sans",
                log.level === "error" && "text-rose-400",
                log.level === "warn" && "text-amber-300",
              )}
            >
              {ts(log.message)}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

function parseManifest(json: string | null): Manifest | null {
  if (!json) return null;
  try {
    return JSON.parse(json) as Manifest;
  } catch {
    return null;
  }
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
  const t = useT();
  const id = Number(useParams().id);
  const { data, isLoading, error } = useRun(id);
  const cancel = useCancelRun();
  const deleteRun = useDeleteRun();
  const deleteArtifact = useDeleteArtifact();
  const pinArtifact = usePinArtifact();
  const confirm = useConfirm();
  const toast = useToast();
  const navigate = useNavigate();
  const now = useNow(1000);
  const [restoreFrom, setRestoreFrom] = useState<Artifact | null>(null);

  if (isLoading) return <LoadingBlock />;
  if (error || !data) {
    return (
      <EmptyState
        icon={<CircleX />}
        title={t("run_detail.not_found")}
        action={<Link to="/runs">{t("run_detail.back")}</Link>}
      />
    );
  }
  const { run, logs, restore_options: restore } = data;
  const active = run.status === "queued" || run.status === "running";

  const onCancel = async () => {
    const { confirmed } = await confirm({
      title: t("run_detail.cancel_title"),
      message: t("run_detail.cancel_message"),
      confirmLabel: t("run_detail.cancel_confirm"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await cancel.mutateAsync(run.id);
    } catch (e) {
      toast.error(t("run_detail.cancel_failed"), errorMessage(e));
    }
  };

  const onDelete = async () => {
    const stored = run.artifacts.filter((a) => a.status === "stored").length;
    const { confirmed, checked } = await confirm({
      title: t("run_detail.delete_title", { id: run.id }),
      message: t("run_detail.delete_message"),
      checkbox: stored > 0 ? t("run_detail.delete_files", { count: stored }) : undefined,
      checkboxDefault: false,
      confirmLabel: t("common.delete"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteRun.mutateAsync({ id: run.id, deleteFiles: checked });
      toast.success(t("run_detail.deleted", { id: run.id }));
      navigate("/runs");
    } catch (e) {
      toast.error(t("common.cannot_delete"), errorMessage(e));
    }
  };

  const onDeleteArtifact = async (artifact: Artifact) => {
    const { confirmed } = await confirm({
      title: t("run_detail.delete_file_title"),
      message: t("run_detail.delete_file_message", { destination: artifact.destination_name }),
      confirmLabel: t("run_detail.delete_file"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteArtifact.mutateAsync(artifact.id);
      toast.success(t("run_detail.file_deleted"));
    } catch (e) {
      toast.error(t("run_detail.delete_file_failed"), errorMessage(e));
    }
  };

  const onPin = async (artifact: Artifact) => {
    try {
      await pinArtifact.mutateAsync({ id: artifact.id, pinned: !artifact.pinned });
      toast.success(artifact.pinned ? t("run_detail.unpinned") : t("run_detail.pinned"), artifact.pinned ? undefined : t("run_detail.pinned_hint"));
    } catch (e) {
      toast.error(t("run_detail.pin_failed"), errorMessage(e));
    }
  };

  const summary: [ReactNode, ReactNode][] = [
    [
      t("run_detail.instance"),
      run.instance_id ? (
        <Link to={`/instances/${run.instance_id}`} className="hover:underline">
          <bdi>{run.instance_name}</bdi>
        </Link>
      ) : (
        <bdi>{run.instance_name}</bdi>
      ),
    ],
    [t("common.database"), <bdi>{run.database}</bdi>],
    [
      run.kind === "restore" ? t("run_detail.source") : t("run_detail.job"),
      run.kind === "restore" && restore ? (
        <Link to={`/runs/${restore.source_run_id}`} className="hover:underline">
          {t("run_detail.source_value", {
            instance: restore.source_instance,
            database: restore.source_database,
            id: restore.source_run_id,
          })}
        </Link>
      ) : run.job_id ? (
        <Link to={`/runs?job_id=${run.job_id}`} className="hover:underline">
          <bdi>{run.job_name}</bdi>
        </Link>
      ) : (
        t("run_detail.manual_backup")
      ),
    ],
    [t("run_detail.trigger"), run.trigger === "schedule" ? t("run_detail.scheduled") : t("run_detail.manual")],
    [t("common.format"), run.format === "zip" ? t("run_detail.format_zip") : t("run_detail.format_dump")],
    [t("run_detail.odoo_version"), run.odoo_version ? <bdi>{run.odoo_version}</bdi> : "—"],
    [t("run_detail.created"), formatDateTime(run.created_at)],
    [t("runs.col_duration"), formatDuration(runDurationMs(run, now))],
    [t("common.size"), formatBytes(run.size_bytes)],
    [t("run_detail.attempts"), run.attempt || "—"],
  ];
  if (restore) {
    summary.push([
      t("run_detail.options"),
      <span className="flex flex-wrap gap-1">
        <Badge tone={restore.copy ? "violet" : "amber"}>
          {restore.copy ? t("run_detail.copy_new_uuid") : t("run_detail.same_uuid")}
        </Badge>
        {restore.neutralize && <Badge tone="blue">{t("run_detail.neutralized")}</Badge>}
      </span>,
    ]);
  }
  const manifest = parseManifest(run.manifest_json);
  if (manifest) {
    const parts =
      manifest.entries === 0
        ? [t("run_detail.contents_dump", { size: formatBytes(manifest.dump_bytes) })]
        : [
            t("run_detail.contents_dump_sql", { size: formatBytes(manifest.dump_bytes) }),
            t("run_detail.contents_files", { count: manifest.filestore_files }),
          ];
    if (manifest.modules != null) parts.push(t("run_detail.contents_modules", { count: manifest.modules }));
    if (manifest.pg_version) parts.push(`PostgreSQL ${manifest.pg_version}`);
    summary.push([t("run_detail.contents"), parts.join(" · ")]);
  }
  if (run.file_name) summary.push([t("run_detail.file"), <Mono>{run.file_name}</Mono>]);
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
          <Link
            to="/runs"
            className="mb-2 inline-flex items-center gap-1 text-sm text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"
          >
            <ArrowLeft className="size-4 rtl:rotate-180" /> {t("nav.history")}
          </Link>
        }
        title={
          <span className="flex items-center gap-3">
            {run.kind === "restore" ? t("run_detail.restore_title", { id: run.id }) : t("run_detail.backup_title", { id: run.id })}
            <StatusBadge status={run.status} />
          </span>
        }
        description={
          <bdi>
            {run.instance_name} / {run.database}
          </bdi>
        }
        actions={
          active ? (
            <Button variant="danger" icon={<Square />} onClick={onCancel} loading={cancel.isPending}>
              {t("common.cancel")}
            </Button>
          ) : (
            <Button icon={<Trash />} onClick={onDelete}>
              {t("common.delete")}
            </Button>
          )
        }
      />

      <div className="space-y-6">
        {run.error && run.status !== "cancelled" && (
          <Alert
            tone={run.status === "partial" ? "amber" : "red"}
            icon={<CircleX />}
            title={run.status === "partial" ? t("run_detail.partial") : t("status.failed")}
          >
            {ts(run.error)}
          </Alert>
        )}

        {run.warning && (
          <Alert tone="amber" icon={<TriangleAlert />} title={t("run_detail.warning")}>
            {run.warning.split("; ").map((w) => (
              <p key={w}>{ts(w)}</p>
            ))}
          </Alert>
        )}

        {active && (
          <Card title={t("run_detail.progress")}>
            <RunProgress run={run} />
          </Card>
        )}

        <div className="grid grid-cols-1 gap-6 xl:grid-cols-5">
          <div className="space-y-6 xl:col-span-2">
            <Card title={t("run_detail.summary")}>
              <KeyValue items={summary} />
            </Card>
          </div>

          <div className="space-y-6 xl:col-span-3">
            {run.kind === "backup" && (
              <Card title={t("run_detail.stored_copies")} padded={false}>
                {run.artifacts.length === 0 ? (
                  <p className="px-5 py-6 text-center text-sm text-zinc-500">{t("run_detail.no_destination")}</p>
                ) : (
                  <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                    {run.artifacts.map((a) => (
                      <li key={a.id} className="flex flex-wrap items-center gap-x-4 gap-y-2 px-5 py-3 sm:flex-nowrap">
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <DestinationIcon kind={a.destination_kind} className="shrink-0 text-zinc-500" />
                            <bdi className="truncate text-sm font-medium">{a.destination_name}</bdi>
                          </div>
                          {a.remote_path && (
                            <p className="mt-0.5 truncate text-xs text-zinc-500" title={a.remote_path} dir="ltr">
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
                              {ts(a.error)}
                            </p>
                          )}
                        </div>
                        {a.pinned && (
                          <Badge tone="violet">
                            <Pin /> {t("run_detail.pinned_badge")}
                          </Badge>
                        )}
                        <Badge tone={artifactTone[a.status]}>
                          {artifactStatusLabel(a.status)}
                          {a.delete_reason ? ` (${deleteReasonLabel(a.delete_reason)})` : ""}
                        </Badge>
                        {a.status === "stored" && (
                          <div className="flex shrink-0 gap-0.5">
                            {a.web_url && (
                              <IconLink label={t("run_detail.open_drive")} href={a.web_url} external>
                                <ExternalLink />
                              </IconLink>
                            )}
                            <IconLink label={t("run_detail.download")} href={`/api/artifacts/${a.id}/download`} download>
                              <Download />
                            </IconLink>
                            <IconButton label={t("run_detail.restore")} onClick={() => setRestoreFrom(a)}>
                              <RotateCcw />
                            </IconButton>
                            <IconButton label={a.pinned ? t("run_detail.unpin") : t("run_detail.pin")} onClick={() => onPin(a)}>
                              {a.pinned ? <PinOff /> : <Pin />}
                            </IconButton>
                            {!a.pinned && (
                              <IconButton label={t("run_detail.delete_file")} tone="danger" onClick={() => onDeleteArtifact(a)}>
                                <Trash />
                              </IconButton>
                            )}
                          </div>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </Card>
            )}

            <Card title={t("run_detail.log")} padded={false}>
              <LogView logs={logs} />
            </Card>
          </div>
        </div>
      </div>

      <RestoreDialog run={run} artifact={restoreFrom} open={restoreFrom !== null} onClose={() => setRestoreFrom(null)} />
    </>
  );
}
