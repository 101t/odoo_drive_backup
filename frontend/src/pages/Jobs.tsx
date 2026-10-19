import { CalendarClock, Pencil, Play, Plus, Trash } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { useDeleteJob, useDestinations, useJobs, useRunJob, useSaveJob } from "../api/hooks";
import type { Job } from "../api/types";
import { useConfirm } from "../components/confirm";
import { DestinationIcon, StatusBadge } from "../components/RunBits";
import { useToast } from "../components/toast";
import { Badge, Button, Card, EmptyState, IconButton, LoadingBlock, Mono, PageHeader, Switch } from "../components/ui";
import { formatDateTime, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";
import { JobForm, jobToInput, retentionSummary } from "./JobForm";

export function JobsPage() {
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
      toast.success(`${job.name} started`, `${run_ids.length} run(s) queued`);
      navigate(run_ids.length === 1 ? `/runs/${run_ids[0]}` : `/runs?job_id=${job.id}`);
    } catch (error) {
      toast.error("Cannot start the job", errorMessage(error));
    }
  };

  const toggle = async (job: Job, enabled: boolean) => {
    try {
      await saveJob.mutateAsync({ id: job.id, input: { ...jobToInput(job), enabled } });
    } catch (error) {
      toast.error("Cannot update the job", errorMessage(error));
    }
  };

  const remove = async (job: Job) => {
    const { confirmed } = await confirm({
      title: `Delete ${job.name}?`,
      message: "The schedule stops. Backups already made are kept and stay in History.",
      confirmLabel: "Delete job",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await deleteJob.mutateAsync(job.id);
      toast.success(`${job.name} deleted`);
    } catch (error) {
      toast.error("Cannot delete", errorMessage(error));
    }
  };

  return (
    <>
      <PageHeader
        title="Backup jobs"
        description="Scheduled backups with multi-destination upload and retention."
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
            New job
          </Button>
        }
      />

      {isLoading ? (
        <LoadingBlock />
      ) : !jobs?.length ? (
        <Card>
          <EmptyState
            icon={<CalendarClock />}
            title="No backup job yet"
            description="A job backs up one or more databases of an instance on a schedule, uploads them to your destinations and prunes old copies."
            action={
              <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
                New job
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="space-y-3">
          {jobs.map((job) => (
            <Card key={job.id} padded={false}>
              <div className="grid gap-4 p-5 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1fr)_auto] lg:items-center">
                <div className="min-w-0 space-y-1.5">
                  <div className="flex items-center gap-2">
                    <p className="truncate font-semibold">{job.name}</p>
                    {!job.enabled && <Badge tone="gray">disabled</Badge>}
                  </div>
                  <p className="text-xs text-zinc-500">
                    <Link to={`/instances/${job.instance_id}`} className="hover:underline">
                      {job.instance_name}
                    </Link>{" "}
                    · {job.format === "zip" ? "ZIP + filestore" : "pg_dump"}
                  </p>
                  <div className="flex flex-wrap gap-1">
                    {job.all_databases ? (
                      <Badge tone="violet">All databases</Badge>
                    ) : (
                      job.databases.map((db) => (
                        <Badge key={db} tone="gray">
                          {db}
                        </Badge>
                      ))
                    )}
                  </div>
                </div>

                <div className="min-w-0 space-y-1 text-sm">
                  <p className="truncate">{job.cron_description || <Mono>{job.cron}</Mono>}</p>
                  <p className="text-xs text-zinc-500" title={formatDateTime(job.next_run_at)}>
                    {job.enabled && job.next_run_at ? `Next ${timeAgo(job.next_run_at, now)}` : "Not scheduled"} ·{" "}
                    {job.timezone}
                  </p>
                  <p className="text-xs text-zinc-500">Retention: {retentionSummary(job)}</p>
                </div>

                <div className="min-w-0 space-y-1.5">
                  <div className="flex flex-wrap gap-1">
                    {job.destination_ids.map((id) => {
                      const d = destinations.find((x) => x.id === id);
                      return d ? (
                        <span key={id} className="inline-flex items-center gap-1 rounded-md bg-zinc-100 px-1.5 py-0.5 text-xs dark:bg-zinc-800">
                          <DestinationIcon kind={d.kind} className="size-3.5" /> {d.name}
                        </span>
                      ) : null;
                    })}
                  </div>
                  <div className="flex items-center gap-2 text-xs text-zinc-500">
                    <StatusBadge status={job.last_status} />
                    {job.last_run_at && <span>{timeAgo(job.last_run_at, now)}</span>}
                  </div>
                </div>

                <div className="flex items-center gap-1 lg:justify-end">
                  <Switch checked={job.enabled} onChange={(v) => toggle(job, v)} />
                  <div className="w-2" />
                  <IconButton label="Run now" onClick={() => run(job)}>
                    <Play />
                  </IconButton>
                  <IconButton label="Edit" onClick={() => openForm(job)}>
                    <Pencil />
                  </IconButton>
                  <IconButton label="Delete" tone="danger" onClick={() => remove(job)}>
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
