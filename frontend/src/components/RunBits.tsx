import { clsx } from "clsx";
import {
  Ban,
  CircleCheck,
  CircleDashed,
  CircleX,
  Clock,
  Cloud,
  HardDrive,
  LoaderCircle,
  TriangleAlert,
} from "lucide-react";
import type { ReactNode } from "react";
import type { Artifact, ArtifactStatus, Run, RunStatus } from "../api/types";
import { t, ts, useT, type MessageKey } from "../i18n";
import { formatBytes, formatRate, phaseLabel } from "../lib/format";
import { useLiveProgress } from "../lib/live";
import { Badge, ProgressBar, type Tone } from "./ui";

const statusMeta: Record<RunStatus, { tone: Tone; label: MessageKey; icon: ReactNode }> = {
  success: { tone: "green", label: "status.success", icon: <CircleCheck /> },
  partial: { tone: "amber", label: "status.partial", icon: <TriangleAlert /> },
  failed: { tone: "red", label: "status.failed", icon: <CircleX /> },
  running: { tone: "blue", label: "status.running", icon: <LoaderCircle className="animate-spin" /> },
  queued: { tone: "gray", label: "status.queued", icon: <Clock /> },
  cancelled: { tone: "gray", label: "status.cancelled", icon: <Ban /> },
};

export function statusLabel(status: RunStatus): string {
  return t(statusMeta[status]?.label ?? "status.queued");
}

const artifactLabels: Record<ArtifactStatus, MessageKey> = {
  pending: "artifact.pending",
  uploading: "artifact.uploading",
  stored: "artifact.stored",
  failed: "artifact.failed",
  skipped: "artifact.skipped",
  deleted: "artifact.deleted",
};

export function artifactStatusLabel(status: ArtifactStatus): string {
  return t(artifactLabels[status] ?? "artifact.pending");
}

export function deleteReasonLabel(reason: string): string {
  if (reason === "retention") return t("artifact.reason_retention");
  if (reason === "manual") return t("artifact.reason_manual");
  return reason;
}

export function StatusBadge({ status }: { status: RunStatus | null | undefined }) {
  const t = useT();
  if (!status) {
    return (
      <Badge tone="gray">
        <CircleDashed /> {t("status.never")}
      </Badge>
    );
  }
  const meta = statusMeta[status];
  return (
    <Badge tone={meta.tone}>
      {meta.icon}
      {t(meta.label)}
    </Badge>
  );
}

export function StatusDot({ status }: { status: RunStatus | null | undefined }) {
  const color =
    status === "success"
      ? "bg-emerald-500"
      : status === "failed"
        ? "bg-rose-500"
        : status === "partial"
          ? "bg-amber-500"
          : status === "running"
            ? "bg-sky-500 animate-pulse"
            : "bg-zinc-300 dark:bg-zinc-600";
  return <span className={clsx("inline-block size-2 shrink-0 rounded-full", color)} />;
}

export function DestinationIcon({ kind, className }: { kind: string; className?: string }) {
  return kind === "gdrive" ? (
    <Cloud className={clsx("size-4", className)} />
  ) : (
    <HardDrive className={clsx("size-4", className)} />
  );
}

export function ArtifactChips({ artifacts }: { artifacts: Artifact[] }) {
  if (artifacts.length === 0) return <span className="text-zinc-400">—</span>;
  return (
    <div className="flex flex-wrap gap-1">
      {artifacts.map((a) => {
        const tone =
          a.status === "stored"
            ? "text-emerald-600 bg-emerald-50 dark:bg-emerald-500/10 dark:text-emerald-400"
            : a.status === "failed"
              ? "text-rose-600 bg-rose-50 dark:bg-rose-500/10 dark:text-rose-400"
              : a.status === "uploading"
                ? "text-sky-600 bg-sky-50 dark:bg-sky-500/10 dark:text-sky-400"
                : "text-zinc-500 bg-zinc-100 dark:bg-zinc-800 dark:text-zinc-400";
        return (
          <span
            key={a.id}
            title={`${a.destination_name}: ${artifactStatusLabel(a.status)}${a.delete_reason ? ` (${deleteReasonLabel(a.delete_reason)})` : ""}${a.error ? ` — ${ts(a.error)}` : ""}`}
            className={clsx(
              "inline-flex max-w-40 items-center gap-1 rounded-md px-1.5 py-0.5 text-xs",
              tone,
              (a.status === "deleted" || a.status === "skipped") && "line-through opacity-70",
            )}
          >
            <DestinationIcon kind={a.destination_kind} className="size-3.5 shrink-0" />
            <span className="truncate">{a.destination_name}</span>
          </span>
        );
      })}
    </div>
  );
}

/** Phase, progress bar and throughput of an active run (live through SSE). */
export function RunProgress({ run, compact = false }: { run: Run; compact?: boolean }) {
  const t = useT();
  const progress = useLiveProgress(run.id, run.progress);
  if (run.status === "queued") {
    return <span className="text-xs text-zinc-500">{t("progress.queued")}</span>;
  }
  if (run.status !== "running") return null;

  const phase = progress?.phase ?? run.phase;
  const total = progress?.bytes_total ?? null;
  const done = progress?.bytes_done ?? 0;
  const ratio = total ? done / total : null;
  const label = [phaseLabel(phase), progress?.label ? t("progress.target", { label: progress.label }) : null]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={clsx("min-w-44 space-y-1", compact ? "max-w-xs" : "")}>
      <div className="flex items-baseline justify-between gap-3 text-xs">
        <span className="truncate font-medium text-sky-700 dark:text-sky-400">{label || t("progress.starting")}</span>
        {ratio !== null && <span className="tabular-nums text-zinc-500">{Math.floor(ratio * 100)}%</span>}
      </div>
      <ProgressBar value={phase === "retention" ? null : ratio} />
      {progress && done > 0 && (
        <div className="flex justify-between gap-3 text-[11px] tabular-nums text-zinc-500">
          <span>
            {formatBytes(done)}
            {total ? ` / ${formatBytes(total)}` : ""}
          </span>
          {progress.rate_bps > 0 && <span>{formatRate(progress.rate_bps)}</span>}
        </div>
      )}
    </div>
  );
}
