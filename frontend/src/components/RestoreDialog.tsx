import { RotateCcw, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { useInstances, useRestore } from "../api/hooks";
import type { Artifact, Run } from "../api/types";
import { useT } from "../i18n";
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
  const t = useT();
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
      toast.success(
        t("restore.started"),
        t("restore.started_hint", { instance: target?.name ?? "", database: database.trim() }),
      );
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
      title={t("restore.title")}
      description={t("restore.description", {
        instance: run.instance_name,
        database: run.database,
        destination: artifact.destination_name,
      })}
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            variant="primary"
            icon={<RotateCcw />}
            loading={restore.isPending}
            disabled={!target || !validName || exists}
            onClick={submit}
          >
            {t("restore.submit")}
          </Button>
        </>
      }
    >
      <div className="space-y-5">
        {restore.error && (
          <Alert tone="red" title={t("restore.failed")}>
            {errorMessage(restore.error)}
          </Alert>
        )}
        <Alert tone="blue" icon={<TriangleAlert />} title={t("restore.never_overwrite")}>
          {t("restore.never_overwrite_hint")}
        </Alert>

        <Field label={t("restore.target")}>
          <Select value={instanceId} onChange={(e) => setInstanceId(Number(e.target.value))}>
            {instances.map((i) => (
              <option key={i.id} value={i.id}>
                {i.odoo_version ? t("restore.target_option", { name: i.name, version: i.odoo_version }) : i.name}
              </option>
            ))}
          </Select>
        </Field>

        <Field
          label={t("restore.name")}
          error={!validName ? t("restore.name_invalid") : exists ? t("restore.name_exists") : undefined}
          hint={
            run.odoo_version &&
            target?.odoo_version &&
            !target.odoo_version.startsWith(run.odoo_version.split(".")[0]) ? (
              <span className="text-amber-600">
                {t("restore.version_mismatch", { source: run.odoo_version, target: target.odoo_version })}
              </span>
            ) : undefined
          }
        >
          <Input value={database} onChange={(e) => setDatabase(e.target.value)} spellCheck={false} dir="ltr" />
        </Field>

        <div className="space-y-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <Switch
            checked={copy}
            onChange={setCopy}
            label={t("restore.copy")}
            description={sameAsSource ? t("restore.copy_same_server") : t("restore.copy_other_server")}
          />
          <Switch
            checked={neutralize}
            onChange={setNeutralize}
            label={t("restore.neutralize")}
            description={t("restore.neutralize_hint")}
          />
        </div>
      </div>
    </Modal>
  );
}
