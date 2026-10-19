import { Cloud, ExternalLink, FlaskConical, KeyRound, Pencil, Plus, Trash } from "lucide-react";
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router";
import { errorMessage } from "../api/client";
import { useDeleteDestination, useDestinations, useStartOAuth, useTestDestination } from "../api/hooks";
import type { Destination, GdriveConfig, LocalConfig, TestReport } from "../api/types";
import { useConfirm } from "../components/confirm";
import { DestinationIcon } from "../components/RunBits";
import { TestReportView } from "../components/TestReportView";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Card, EmptyState, IconButton, LoadingBlock, Modal, Mono, PageHeader, Spinner } from "../components/ui";
import { formatBytes } from "../lib/format";
import { DestinationForm } from "./DestinationForm";

function Details({ destination }: { destination: Destination }) {
  if (destination.kind === "local") {
    const c = destination.config as LocalConfig;
    return (
      <p className="truncate text-xs text-zinc-500">
        <Mono>{c.path}</Mono>
      </p>
    );
  }
  const c = destination.config as GdriveConfig;
  const account = c.auth === "service_account" ? c.service_account_email : c.account_email;
  return (
    <div className="space-y-1 text-xs text-zinc-500">
      <p className="truncate">
        {c.auth === "service_account" ? "Service account" : "OAuth"}
        {account ? ` · ${account}` : ""}
        {c.impersonate ? ` → ${c.impersonate}` : ""}
      </p>
      {c.folder_id ? (
        <a
          href={`https://drive.google.com/drive/folders/${c.folder_id}`}
          target="_blank"
          rel="noreferrer"
          className="inline-flex max-w-full items-center gap-1 hover:text-brand-600"
        >
          <span className="truncate">Folder {c.folder_id}</span>
          <ExternalLink className="size-3 shrink-0" />
        </a>
      ) : (
        <p>My Drive / Odoo Backups</p>
      )}
    </div>
  );
}

export function DestinationsPage() {
  const { data: destinations, isLoading } = useDestinations();
  const remove = useDeleteDestination();
  const test = useTestDestination();
  const startOAuth = useStartOAuth();
  const confirm = useConfirm();
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const [editing, setEditing] = useState<Destination | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [testing, setTesting] = useState<{ destination: Destination; report: TestReport | null } | null>(null);

  useEffect(() => {
    const connected = params.get("connected");
    const oauthError = params.get("oauth_error");
    if (connected) toast.success("Google account connected", "Run a test to confirm uploads work.");
    if (oauthError) toast.error("Google authorization failed", oauthError);
    if (connected || oauthError) setParams({}, { replace: true });
  }, [params, setParams, toast]);

  const openForm = (destination: Destination | null) => {
    setEditing(destination);
    setFormOpen(true);
  };

  const runTest = async (destination: Destination) => {
    setTesting({ destination, report: null });
    try {
      const report = await test.mutateAsync(destination.id);
      setTesting({ destination, report });
    } catch (error) {
      setTesting({ destination, report: { ok: false, steps: [{ name: "Request", status: "fail", detail: errorMessage(error) }] } });
    }
  };

  const connect = async (destination: Destination) => {
    try {
      const { url } = await startOAuth.mutateAsync(destination.id);
      window.location.href = url;
    } catch (error) {
      toast.error("Cannot start the Google authorization", errorMessage(error));
    }
  };

  const del = async (destination: Destination) => {
    const { confirmed } = await confirm({
      title: `Delete ${destination.name}?`,
      message: "Files already stored there are not deleted, but they can no longer be downloaded or restored from this app.",
      confirmLabel: "Delete destination",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await remove.mutateAsync(destination.id);
      toast.success(`${destination.name} deleted`);
    } catch (error) {
      toast.error("Cannot delete", errorMessage(error));
    }
  };

  return (
    <>
      <PageHeader
        title="Destinations"
        description="Google Drive folders and local/NAS directories receiving the backups."
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
            Add destination
          </Button>
        }
      />

      {isLoading ? (
        <LoadingBlock />
      ) : !destinations?.length ? (
        <Card>
          <EmptyState
            icon={<Cloud />}
            title="No destination yet"
            description="Store backups on Google Drive (Shared Drive with a service account, or your own account via OAuth) and/or on a local disk or NAS mount. Keep at least one copy off-site."
            action={
              <Button variant="primary" icon={<Plus />} onClick={() => openForm(null)}>
                Add destination
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {destinations.map((destination) => {
            const oauth = destination.kind === "gdrive" && (destination.config as GdriveConfig).auth === "oauth";
            return (
              <Card key={destination.id} padded={false} className="flex flex-col">
                <div className="flex-1 space-y-3 p-5">
                  <div className="flex items-start gap-3">
                    <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-brand-50 text-brand-600 dark:bg-brand-500/10 dark:text-brand-300">
                      <DestinationIcon kind={destination.kind} className="size-5" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-semibold">{destination.name}</p>
                      <Details destination={destination} />
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    <Badge tone="gray">{destination.kind === "gdrive" ? "Google Drive" : "Local folder"}</Badge>
                    {oauth && !destination.connected && <Badge tone="amber">Not connected</Badge>}
                    {oauth && destination.connected && <Badge tone="green">Connected</Badge>}
                    {destination.job_count > 0 && <Badge tone="violet">{destination.job_count} job(s)</Badge>}
                  </div>
                  <p className="text-sm text-zinc-600 dark:text-zinc-300">
                    <span className="font-semibold tabular-nums">{formatBytes(destination.stored_bytes)}</span> in{" "}
                    {destination.stored_files} backup file(s)
                  </p>
                </div>
                <div className="flex items-center gap-1 border-t border-zinc-100 px-3 py-2 dark:border-zinc-800">
                  <Button size="sm" variant="ghost" icon={<FlaskConical />} onClick={() => runTest(destination)}>
                    Test
                  </Button>
                  {oauth && (
                    <Button size="sm" variant="ghost" icon={<KeyRound />} onClick={() => connect(destination)}>
                      {destination.connected ? "Reconnect" : "Connect"}
                    </Button>
                  )}
                  <div className="flex-1" />
                  <IconButton label="Edit" onClick={() => openForm(destination)}>
                    <Pencil />
                  </IconButton>
                  <IconButton label="Delete" tone="danger" onClick={() => del(destination)}>
                    <Trash />
                  </IconButton>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <DestinationForm destination={editing} open={formOpen} onClose={() => setFormOpen(false)} />

      <Modal
        open={testing !== null}
        onClose={() => setTesting(null)}
        title={`Testing ${testing?.destination.name ?? ""}`}
        description="Authenticates, checks the folder and writes then deletes a small file."
        footer={<Button onClick={() => setTesting(null)}>Close</Button>}
      >
        {testing?.report ? (
          <div className="space-y-4">
            <Alert tone={testing.report.ok ? "green" : "red"} title={testing.report.ok ? "Destination works" : "The destination is not usable"} />
            <TestReportView report={testing.report} />
          </div>
        ) : (
          <div className="flex items-center gap-3 py-6 text-sm text-zinc-500">
            <Spinner /> Running checks…
          </div>
        )}
      </Modal>
    </>
  );
}
