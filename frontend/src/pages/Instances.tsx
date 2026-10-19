import { ExternalLink, Pencil, Play, Plus, RefreshCw, Server, ShieldAlert, Trash } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { errorMessage } from "../api/client";
import { useDeleteInstance, useInstances, useRefreshInstance } from "../api/hooks";
import type { Instance } from "../api/types";
import { BackupNowDialog } from "../components/BackupNowDialog";
import { useConfirm } from "../components/confirm";
import { StatusDot } from "../components/RunBits";
import { useToast } from "../components/toast";
import { Badge, Button, Card, EmptyState, IconButton, LoadingBlock, PageHeader } from "../components/ui";
import { timeAgo } from "../lib/format";
import { InstanceForm } from "./InstanceForm";

export function useInstanceActions() {
  const refresh = useRefreshInstance();
  const remove = useDeleteInstance();
  const confirm = useConfirm();
  const toast = useToast();

  return {
    refreshing: refresh.isPending ? refresh.variables : undefined,
    refresh: async (instance: Instance) => {
      try {
        const result = await refresh.mutateAsync(instance.id);
        if (result.test.error) toast.error(`${instance.name} is unreachable`, result.test.error);
        else toast.success(`${instance.name} refreshed`, `Odoo ${result.test.version} · ${result.test.databases.length} database(s)`);
      } catch (error) {
        toast.error("Refresh failed", errorMessage(error));
      }
    },
    remove: async (instance: Instance) => {
      const { confirmed } = await confirm({
        title: `Delete ${instance.name}?`,
        message: (
          <p>
            Its {instance.job_count} backup job(s) are deleted too. Existing backup files are kept on their
            destinations and remain visible in History.
          </p>
        ),
        confirmLabel: "Delete instance",
        danger: true,
      });
      if (!confirmed) return false;
      try {
        await remove.mutateAsync(instance.id);
        toast.success(`${instance.name} deleted`);
        return true;
      } catch (error) {
        toast.error("Cannot delete", errorMessage(error));
        return false;
      }
    },
  };
}

export function InstancesPage() {
  const { data: instances, isLoading } = useInstances();
  const [editing, setEditing] = useState<Instance | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [backup, setBackup] = useState<Instance | null>(null);
  const actions = useInstanceActions();

  const openForm = (instance: Instance | null) => {
    setEditing(instance);
    setFormOpen(true);
  };

  return (
    <>
      <PageHeader
        title="Instances"
        description="Odoo servers to back up. Databases are discovered automatically."
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
            Add instance
          </Button>
        }
      />

      {isLoading ? (
        <LoadingBlock />
      ) : !instances?.length ? (
        <Card>
          <EmptyState
            icon={<Server />}
            title="No Odoo instance yet"
            description="Add the URL of an Odoo server and its master password. Odoo 12 to 18 (Community and Enterprise) are supported, as long as the database manager is enabled."
            action={
              <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
                Add instance
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {instances.map((instance) => (
            <Card key={instance.id} className="flex flex-col" padded={false}>
              <div className="flex-1 space-y-3 p-5">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <Link to={`/instances/${instance.id}`} className="block truncate font-semibold hover:underline">
                      {instance.name}
                    </Link>
                    <a
                      href={instance.url}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex max-w-full items-center gap-1 truncate text-xs text-zinc-500 hover:text-brand-600"
                    >
                      <span className="truncate">{instance.url}</span>
                      <ExternalLink className="size-3 shrink-0" />
                    </a>
                  </div>
                  {instance.odoo_version && <Badge tone="violet">Odoo {instance.odoo_version}</Badge>}
                </div>

                <div className="flex flex-wrap gap-1.5">
                  {!instance.has_master_password && (
                    <Badge tone="amber">
                      <ShieldAlert /> No master password
                    </Badge>
                  )}
                  {!instance.verify_tls && <Badge tone="amber">TLS not verified</Badge>}
                </div>

                {instance.last_check_error && (
                  <p className="line-clamp-2 text-xs text-rose-600 dark:text-rose-400" title={instance.last_check_error}>
                    {instance.last_check_error}
                  </p>
                )}

                <dl className="grid grid-cols-3 gap-2 rounded-lg bg-zinc-50 p-3 text-center dark:bg-zinc-950/50">
                  <div>
                    <dt className="text-[11px] uppercase tracking-wide text-zinc-500">Databases</dt>
                    <dd className="text-lg font-semibold tabular-nums">{instance.databases.length}</dd>
                  </div>
                  <div>
                    <dt className="text-[11px] uppercase tracking-wide text-zinc-500">Jobs</dt>
                    <dd className="text-lg font-semibold tabular-nums">{instance.job_count}</dd>
                  </div>
                  <div>
                    <dt className="text-[11px] uppercase tracking-wide text-zinc-500">Last backup</dt>
                    <dd className="mt-1.5 flex items-center justify-center gap-1.5 text-xs">
                      <StatusDot status={instance.last_backup_status} />
                      {timeAgo(instance.last_backup_at)}
                    </dd>
                  </div>
                </dl>
              </div>
              <div className="flex items-center gap-1 border-t border-zinc-100 px-3 py-2 dark:border-zinc-800">
                <Button size="sm" variant="ghost" icon={<Play />} onClick={() => setBackup(instance)}>
                  Back up now
                </Button>
                <div className="flex-1" />
                <IconButton
                  label="Refresh databases"
                  onClick={() => actions.refresh(instance)}
                  disabled={actions.refreshing === instance.id}
                >
                  <RefreshCw className={actions.refreshing === instance.id ? "animate-spin" : undefined} />
                </IconButton>
                <IconButton label="Edit" onClick={() => openForm(instance)}>
                  <Pencil />
                </IconButton>
                <IconButton label="Delete" tone="danger" onClick={() => actions.remove(instance)}>
                  <Trash />
                </IconButton>
              </div>
            </Card>
          ))}
        </div>
      )}

      <InstanceForm instance={editing} open={formOpen} onClose={() => setFormOpen(false)} />
      <BackupNowDialog instance={backup} open={backup !== null} onClose={() => setBackup(null)} />
    </>
  );
}
