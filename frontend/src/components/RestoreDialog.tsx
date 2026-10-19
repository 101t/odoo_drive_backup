import { RotateCcw, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { useInstances, useRestore } from "../api/hooks";
import type { Artifact, Run } from "../api/types";
import { useToast } from "./toast";
import { Alert, Button, Field, Input, Modal, Select, Switch } from "./ui";

function defaultName(run: Run) {
  const stamp = new Date().toISOString().slice(0, 10).replaceAll("-", "");
  return `${run.database}_restore_${stamp}`.slice(0, 63);
}

export function RestoreDialog({
  run,
  artifact,
  open,
  onClose,
}: {
  run: Run;
  artifact: Artifact | null;
  open: boolean;
  onClose: () => void;
}) {
  const { data: instances = [] } = useInstances();
  const restore = useRestore();
  const toast = useToast();
  const navigate = useNavigate();
  const [instanceId, setInstanceId] = useState<number>(run.instance_id ?? 0);
  const [database, setDatabase] = useState(defaultName(run));
  const [copy, setCopy] = useState(true);
  const [neutralize, setNeutralize] = useState(false);

  useEffect(() => {
    if (!open) return;
    setInstanceId(run.instance_id ?? instances[0]?.id ?? 0);
    setDatabase(defaultName(run));
    setCopy(true);
    setNeutralize(false);
    restore.reset();
  }, [open, run.id]);

  if (!artifact) return null;
  const target = instances.find((i) => i.id === instanceId);
  const exists = target?.databases.includes(database.trim()) ?? false;
  const validName = /^[a-zA-Z0-9][a-zA-Z0-9_.-]+$/.test(database.trim());
  const sameAsSource = target?.id === run.instance_id;

  const submit = async () => {
    try {
      const result = await restore.mutateAsync({
        artifactId: artifact.id,
        instance_id: instanceId,
        database: database.trim(),
        copy,
        neutralize,
      });
      toast.success("Restore started", `Restoring into ${target?.name} as "${database.trim()}"`);
      onClose();
      navigate(`/runs/${result.run_id}`);
    } catch {
      // shown inline
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Restore backup"
      description={`${run.instance_name} / ${run.database} — from ${artifact.destination_name}`}
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            icon={<RotateCcw />}
            loading={restore.isPending}
            disabled={!target || !validName || exists}
            onClick={submit}
          >
            Restore as new database
          </Button>
        </>
      }
    >
      <div className="space-y-5">
        {restore.error && <Alert tone="red" title="Could not start the restore">{errorMessage(restore.error)}</Alert>}
        <Alert tone="blue" icon={<TriangleAlert />} title="Restores never overwrite">
          The backup is restored into a new database through Odoo's database manager. Existing databases are left
          untouched. Use this to test your backups regularly.
        </Alert>

        <Field label="Target instance">
          <Select value={instanceId} onChange={(e) => setInstanceId(Number(e.target.value))}>
            {instances.map((i) => (
              <option key={i.id} value={i.id}>
                {i.name} {i.odoo_version ? `(Odoo ${i.odoo_version})` : ""}
              </option>
            ))}
          </Select>
        </Field>

        <Field
          label="New database name"
          error={
            !validName
              ? "Use letters, digits, '_', '-' or '.', starting with a letter or digit"
              : exists
                ? "A database with this name already exists on the target"
                : undefined
          }
          hint={run.odoo_version && target?.odoo_version && !target.odoo_version.startsWith(run.odoo_version.split(".")[0]) ? (
            <span className="text-amber-600">
              The backup comes from Odoo {run.odoo_version}, the target runs {target.odoo_version}.
            </span>
          ) : undefined}
        >
          <Input value={database} onChange={(e) => setDatabase(e.target.value)} spellCheck={false} />
        </Field>

        <div className="space-y-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <Switch
            checked={copy}
            onChange={setCopy}
            label="This database is a copy"
            description={
              sameAsSource
                ? "Recommended on the same server: generates a new database UUID so the copy does not conflict with the original (enterprise subscription, mail gateways…)."
                : "Generates a new database UUID. Turn off only when this restore replaces the original server (disaster recovery)."
            }
          />
          <Switch
            checked={neutralize}
            onChange={setNeutralize}
            label="Neutralize (Odoo 16+)"
            description="Disables outgoing mail servers, scheduled actions, payment providers and other integrations in the restored database."
          />
        </div>
      </div>
    </Modal>
  );
}
