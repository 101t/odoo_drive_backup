import { Play } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { useBackupNow, useDestinations } from "../api/hooks";
import type { BackupFormat, Instance } from "../api/types";
import { DestinationIcon } from "./RunBits";
import { useToast } from "./toast";
import { Alert, Button, Checkbox, Field, Modal, Segmented } from "./ui";

export function BackupNowDialog({
  instance,
  preselected,
  open,
  onClose,
}: {
  instance: Instance | null;
  preselected?: string[];
  open: boolean;
  onClose: () => void;
}) {
  const { data: destinations = [] } = useDestinations();
  const backup = useBackupNow();
  const toast = useToast();
  const navigate = useNavigate();
  const [databases, setDatabases] = useState<string[]>([]);
  const [destinationIds, setDestinationIds] = useState<number[]>([]);
  const [format, setFormat] = useState<BackupFormat>("zip");

  useEffect(() => {
    if (!open) return;
    setDatabases(preselected ?? (instance?.databases.length === 1 ? instance.databases : []));
    setFormat("zip");
    backup.reset();
  }, [open, instance?.id]);

  useEffect(() => {
    if (open && destinationIds.length === 0 && destinations.length > 0) {
      setDestinationIds([destinations[0].id]);
    }
  }, [open, destinations, destinationIds.length]);

  if (!instance) return null;
  const available = Array.from(new Set([...(instance.databases ?? []), ...(preselected ?? [])])).sort();

  const toggle = <T,>(list: T[], value: T) => (list.includes(value) ? list.filter((v) => v !== value) : [...list, value]);

  const submit = async () => {
    try {
      const result = await backup.mutateAsync({
        instance_id: instance.id,
        databases,
        destination_ids: destinationIds,
        format,
      });
      toast.success(
        result.run_ids.length === 1 ? "Backup started" : `${result.run_ids.length} backups started`,
        "Follow the progress live in History.",
      );
      onClose();
      navigate(result.run_ids.length === 1 ? `/runs/${result.run_ids[0]}` : "/runs?status=active");
    } catch {
      // error shown in the dialog
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`Back up ${instance.name} now`}
      description="Runs immediately, independent of any schedule. Manual backups are never pruned by retention."
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            icon={<Play />}
            loading={backup.isPending}
            disabled={databases.length === 0 || destinationIds.length === 0}
            onClick={submit}
          >
            Start backup
          </Button>
        </>
      }
    >
      <div className="space-y-5">
        {backup.error && <Alert tone="red" title="Could not start the backup">{errorMessage(backup.error)}</Alert>}
        {!instance.has_master_password && (
          <Alert tone="amber" title="No master password">
            Set the Odoo master password on this instance before backing it up.
          </Alert>
        )}

        <div className="space-y-2">
          <p className="text-sm font-medium">Databases</p>
          {available.length === 0 ? (
            <p className="text-sm text-zinc-500">
              No database known yet. Refresh the instance to list its databases.
            </p>
          ) : (
            <div className="grid max-h-56 grid-cols-1 gap-2 overflow-y-auto rounded-lg border border-zinc-200 p-3 sm:grid-cols-2 dark:border-zinc-800">
              {available.map((db) => (
                <Checkbox key={db} checked={databases.includes(db)} onChange={() => setDatabases(toggle(databases, db))} label={db} />
              ))}
            </div>
          )}
        </div>

        <div className="space-y-2">
          <p className="text-sm font-medium">Destinations</p>
          {destinations.length === 0 ? (
            <p className="text-sm text-zinc-500">Create a destination first (Destinations page).</p>
          ) : (
            <div className="space-y-2">
              {destinations.map((d) => (
                <Checkbox
                  key={d.id}
                  checked={destinationIds.includes(d.id)}
                  onChange={() => setDestinationIds(toggle(destinationIds, d.id))}
                  label={
                    <span className="inline-flex items-center gap-1.5">
                      <DestinationIcon kind={d.kind} className="text-zinc-500" />
                      {d.name}
                    </span>
                  }
                />
              ))}
            </div>
          )}
        </div>

        <Field label="Format" hint={format === "zip" ? "Database dump and filestore (attachments). Recommended." : "PostgreSQL dump only, without the filestore."}>
          <div>
            <Segmented
              value={format}
              onChange={setFormat}
              options={[
                { value: "zip", label: "ZIP (with filestore)" },
                { value: "dump", label: "pg_dump only" },
              ]}
            />
          </div>
        </Field>
      </div>
    </Modal>
  );
}
