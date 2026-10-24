import { CalendarClock, Pencil, Play, Plus, ShieldAlert, ShieldCheck, Trash } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { useDeleteJob, useDestinations, useJobs, useRunJob, useSaveJob } from "../api/hooks";
import type { Job, PolicyScore } from "../api/types";
import { useConfirm } from "../components/confirm";
import { DestinationIcon, StatusBadge } from "../components/RunBits";
import { useToast } from "../components/toast";
import { Badge, Button, Card, EmptyState, IconButton, LoadingBlock, Mono, PageHeader, Switch } from "../components/ui";
import { useT } from "../i18n";
import { formatDateTime, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";
import { JobForm, jobToInput, retentionSummary } from "./JobForm";

/** 3-2-1-1-0 badge; the tooltip lists what is missing and how to fix it. */
function PolicyBadge({ policy }: { policy: PolicyScore }) {
  const t = useT();
  const missing = policy.rules.filter((r) => !r.ok);
  const tone = policy.score === policy.max ? "green" : policy.score >= 3 ? "amber" : "red";
  const tip = [t("jobs.policy_title"), ...missing.map((r) => `• ${t(`policy.${r.id}`)}`)].join("\n");
  return (
    <span title={tip} className="cursor-help">
      <Badge tone={tone}>
        {missing.length === 0 ? <ShieldCheck /> : <ShieldAlert />}
        {t("jobs.policy", { score: policy.score, max: policy.max })}
      </Badge>
    </span>
  );
}

export function JobsPage() {
  const t = useT();
  const { data: jobs, isLoading } = useJobs();
  const { data: destinations = [] } = useDestinations();
  const runJob = useRunJob();
  const saveJob = useSaveJob();
  const deleteJob = useDeleteJob();
  const confirm = useConfirm();
  const toast = useToast();
  const navigate = useNavigate();
  const now = useNow(30_000);
  const [editing, setEditing] = useState<Job | null>(null);
  const [formOpen, setFormOpen] = useState(false);

  const openForm = (job: Job | null) => {
    setEditing(job);
    setFormOpen(true);
  };

  const run = async (job: Job) => {
    try {
      const { run_ids } = await runJob.mutateAsync(job.id);
      toast.success(t("jobs.started", { name: job.name }), t("jobs.started_hint", { count: run_ids.length }));
      navigate(run_ids.length === 1 ? `/runs/${run_ids[0]}` : `/runs?job_id=${job.id}`);
    } catch (error) {
      toast.error(t("jobs.start_failed"), errorMessage(error));
    }
  };

  const toggle = async (job: Job, enabled: boolean) => {
    try {
      await saveJob.mutateAsync({ id: job.id, input: { ...jobToInput(job), enabled } });
    } catch (error) {
      toast.error(t("jobs.update_failed"), errorMessage(error));
    }
  };

  const remove = async (job: Job) => {
    const { confirmed } = await confirm({
      title: t("jobs.delete_title", { name: job.name }),
      message: t("jobs.delete_message"),
      confirmLabel: t("jobs.delete_confirm"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteJob.mutateAsync(job.id);
      toast.success(t("jobs.deleted", { name: job.name }));
    } catch (error) {
      toast.error(t("common.cannot_delete"), errorMessage(error));
    }
  };

  return (
    <>
      <PageHeader
        title={t("jobs.title")}
        description={t("jobs.description")}
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
            {t("jobs.new")}
          </Button>
        }
      />

      {isLoading ? (
        <LoadingBlock />
      ) : !jobs?.length ? (
        <Card>
          <EmptyState
            icon={<CalendarClock />}
            title={t("jobs.empty")}
            description={t("jobs.empty_hint")}
            action={
              <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
                {t("jobs.new")}
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="space-y-3">
          {jobs.map((job) => (
            <Card key={job.id} padded={false}>
              <div className="grid grid-cols-1 gap-4 p-5 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1fr)_auto] lg:items-center">
                <div className="min-w-0 space-y-1.5">
                  <div className="flex items-center gap-2">
                    <p className="truncate font-semibold">
                      <bdi>{job.name}</bdi>
                    </p>
                    {!job.enabled && <Badge tone="gray">{t("jobs.disabled")}</Badge>}
                  </div>
                  <p className="text-xs text-zinc-500">
                    <Link to={`/instances/${job.instance_id}`} className="hover:underline">
                      <bdi>{job.instance_name}</bdi>
                    </Link>{" "}
                    · {job.format === "zip" ? t("jobs.format_zip") : t("jobs.format_dump")}
                  </p>
                  <div className="flex flex-wrap gap-1">
                    {job.all_databases ? (
                      <Badge tone="violet">{t("jobs.all_databases")}</Badge>
                    ) : (
                      job.databases.map((db) => (
                        <Badge key={db} tone="gray">
                          <bdi>{db}</bdi>
                        </Badge>
                      ))
                    )}
                  </div>
                </div>

                <div className="min-w-0 space-y-1 text-sm">
                  <p className="truncate">{job.cron_description || <Mono>{job.cron}</Mono>}</p>
                  <p className="text-xs text-zinc-500" title={formatDateTime(job.next_run_at)}>
                    {job.enabled && job.next_run_at
                      ? t("jobs.next", { when: timeAgo(job.next_run_at, now) })
                      : t("jobs.not_scheduled")}{" "}
                    · <bdi>{job.timezone}</bdi>
                  </p>
                  <p className="text-xs text-zinc-500">{t("jobs.retention", { summary: retentionSummary(job) })}</p>
                </div>

                <div className="min-w-0 space-y-1.5">
                  <div className="flex flex-wrap gap-1">
                    {job.destination_ids.map((id) => {
                      const d = destinations.find((x) => x.id === id);
                      return d ? (
                        <span
                          key={id}
                          className="inline-flex items-center gap-1 rounded-md bg-zinc-100 px-1.5 py-0.5 text-xs dark:bg-zinc-800"
                        >
                          <DestinationIcon kind={d.kind} className="size-3.5" /> <bdi>{d.name}</bdi>
                        </span>
                      ) : null;
                    })}
                  </div>
                  <div className="flex items-center gap-2 text-xs text-zinc-500">
                    <StatusBadge status={job.last_status} />
                    <PolicyBadge policy={job.policy} />
                    {job.last_run_at && <span>{timeAgo(job.last_run_at, now)}</span>}
                  </div>
                </div>

                <div className="flex items-center gap-1 lg:justify-end">
                  <Switch checked={job.enabled} onChange={(v) => toggle(job, v)} />
                  <div className="w-2" />
                  <IconButton label={t("jobs.run_now")} onClick={() => run(job)}>
                    <Play />
                  </IconButton>
                  <IconButton label={t("common.edit")} onClick={() => openForm(job)}>
                    <Pencil />
                  </IconButton>
                  <IconButton label={t("common.delete")} tone="danger" onClick={() => remove(job)}>
                    <Trash />
                  </IconButton>
                </div>
              </div>
            </Card>
          ))}
        </div>
      )}

      <JobForm job={editing} open={formOpen} onClose={() => setFormOpen(false)} />
    </>
  );
}
