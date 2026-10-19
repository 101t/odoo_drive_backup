import { clsx } from "clsx";
import { ArrowLeft, CircleX, ExternalLink, Pencil, Play, RefreshCw, Trash } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useInstance, useRuns } from "../api/hooks";
import { BackupNowDialog } from "../components/BackupNowDialog";
import { ArtifactChips, RunProgress, StatusBadge } from "../components/RunBits";
import { Alert, Badge, Button, Card, EmptyState, IconButton, LoadingBlock, PageHeader, tableClass } from "../components/ui";
import { formatBytes, formatDateTime, timeAgo } from "../lib/format";
import { useNow } from "../lib/live";
import { InstanceForm } from "./InstanceForm";
import { useInstanceActions } from "./Instances";

export function InstanceDetailPage() {
  const id = Number(useParams().id);
  const { data: instance, isLoading, error } = useInstance(id);
  const { data: runs } = useRuns({ instance_id: id, limit: 10 });
  const navigate = useNavigate();
  const now = useNow(30_000);
  const actions = useInstanceActions();
  const [editOpen, setEditOpen] = useState(false);
  const [backup, setBackup] = useState<string[] | null>(null);

  if (isLoading) return <LoadingBlock />;
  if (error || !instance) {
    return <EmptyState icon={<CircleX />} title="Instance not found" action={<Link to="/instances">Back to instances</Link>} />;
  }

  return (
    <>
      <PageHeader
        back={
          <Link to="/instances" className="mb-2 inline-flex items-center gap-1 text-sm text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100">
            <ArrowLeft className="size-4" /> Instances
          </Link>
        }
        title={
          <span className="flex items-center gap-3">
            {instance.name}
            {instance.odoo_version && <Badge tone="violet">Odoo {instance.odoo_version}</Badge>}
          </span>
        }
        description={
          <a href={instance.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 hover:text-brand-600">
            {instance.url} <ExternalLink className="size-3" />
          </a>
        }
        actions={
          <>
            <Button
              icon={<RefreshCw className={actions.refreshing === id ? "animate-spin" : undefined} />}
              onClick={() => actions.refresh(instance)}
              disabled={actions.refreshing === id}
            >
              Refresh
            </Button>
            <Button icon={<Pencil />} onClick={() => setEditOpen(true)}>
              Edit
            </Button>
            <Button
              variant="danger"
              icon={<Trash />}
              onClick={async () => {
                if (await actions.remove(instance)) navigate("/instances");
              }}
            >
              Delete
            </Button>
            <Button variant="primary" icon={<Play />} onClick={() => setBackup([])}>
              Back up now
            </Button>
          </>
        }
      />

      <div className="space-y-6">
        {instance.last_check_error && (
          <Alert tone="red" icon={<CircleX />} title="Last check failed">
            {instance.last_check_error}
            {instance.last_checked_at && <span className="ml-1 opacity-70">({timeAgo(instance.last_checked_at, now)})</span>}
          </Alert>
        )}
        {instance.notes && (
          <Card>
            <p className="whitespace-pre-wrap text-sm text-zinc-600 dark:text-zinc-300">{instance.notes}</p>
          </Card>
        )}

        <Card
          title="Databases"
          description={
            instance.last_checked_at ? `List refreshed ${timeAgo(instance.last_checked_at, now)}` : "Never refreshed"
          }
          padded={false}
        >
          {instance.database_details.length === 0 ? (
            <p className="px-5 py-8 text-center text-sm text-zinc-500">No database found. Click Refresh.</p>
          ) : (
            <div className={tableClass.wrapper}>
              <table className={tableClass.table}>
                <thead className={tableClass.head}>
                  <tr>
                    <th className={tableClass.th}>Database</th>
                    <th className={tableClass.th}>Last success</th>
                    <th className={clsx(tableClass.th, "text-right")}>Size</th>
                    <th className={tableClass.th}>Last run</th>
                    <th className={tableClass.th}>Jobs</th>
                    <th className={tableClass.th} />
                  </tr>
                </thead>
                <tbody>
                  {instance.database_details.map((db) => (
                    <tr key={db.name} className={tableClass.row}>
                      <td className={tableClass.td}>
                        <span className="font-medium">{db.name}</span>
                        {!db.exists && (
                          <Badge tone="gray" className="ml-2">
                            not on server
                          </Badge>
                        )}
                      </td>
                      <td className={clsx(tableClass.td, "whitespace-nowrap")} title={formatDateTime(db.last_success_at)}>
                        {timeAgo(db.last_success_at, now)}
                      </td>
                      <td className={clsx(tableClass.td, "text-right tabular-nums")}>{formatBytes(db.last_size)}</td>
                      <td className={tableClass.td}>
                        <StatusBadge status={db.last_status} />
                      </td>
                      <td className={tableClass.td}>
                        <div className="flex flex-wrap gap-1">
                          {db.jobs.length === 0 ? (
                            <Badge tone="amber">not scheduled</Badge>
                          ) : (
                            db.jobs.map((job) => (
                              <Badge key={job.id} tone={job.enabled ? "violet" : "gray"}>
                                {job.name}
                              </Badge>
                            ))
                          )}
                        </div>
                      </td>
                      <td className={clsx(tableClass.td, "text-right")}>
                        {db.exists && (
                          <IconButton label={`Back up ${db.name} now`} onClick={() => setBackup([db.name])}>
                            <Play />
                          </IconButton>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>

        <Card
          title="Recent runs"
          padded={false}
          actions={
            <Link to={`/runs?instance_id=${id}`} className="text-xs font-medium text-brand-600 hover:underline dark:text-brand-400">
              View all
            </Link>
          }
        >
          {!runs?.items.length ? (
            <p className="px-5 py-8 text-center text-sm text-zinc-500">No run yet.</p>
          ) : (
            <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
              {runs.items.map((run) => (
                <li key={run.id}>
                  <Link
                    to={`/runs/${run.id}`}
                    className="flex flex-col gap-2 px-5 py-3 hover:bg-zinc-50 sm:flex-row sm:items-center dark:hover:bg-zinc-800/40"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">
                        {run.kind === "restore" ? "Restore → " : ""}
                        {run.database}
                      </p>
                      <p className="text-xs text-zinc-500">
                        #{run.id} · {timeAgo(run.created_at, now)} · {formatBytes(run.size_bytes)}
                      </p>
                    </div>
                    {run.status === "running" || run.status === "queued" ? (
                      <div className="sm:w-64">
                        <RunProgress run={run} />
                      </div>
                    ) : (
                      <ArtifactChips artifacts={run.artifacts} />
                    )}
                    <StatusBadge status={run.status} />
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>

      <InstanceForm instance={instance} open={editOpen} onClose={() => setEditOpen(false)} />
      <BackupNowDialog
        instance={instance}
        preselected={backup && backup.length > 0 ? backup : undefined}
        open={backup !== null}
        onClose={() => setBackup(null)}
      />
    </>
  );
}
