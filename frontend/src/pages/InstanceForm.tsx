import { CircleCheck, CircleX, Eye, EyeOff, PlugZap, TriangleAlert } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveInstance, useTestInstance } from "../api/hooks";
import type { Instance, InstanceTest } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Field, IconButton, Input, Modal, Switch, Textarea } from "../components/ui";

function TestResult({ result }: { result: InstanceTest }) {
  if (result.error) {
    return (
      <Alert tone="red" icon={<CircleX />} title="Connection failed">
        {result.error}
      </Alert>
    );
  }
  const passwordLine = {
    valid: <span className="text-emerald-700 dark:text-emerald-400">Master password accepted</span>,
    invalid: <span className="font-medium text-rose-700 dark:text-rose-400">Master password rejected by Odoo</span>,
    unknown: <span>Master password could not be verified on this Odoo version</span>,
    missing: <span>No master password provided</span>,
  }[result.master_password];
  return (
    <Alert
      tone={result.ok && !result.list_error ? "green" : "amber"}
      icon={result.ok ? <CircleCheck /> : <TriangleAlert />}
      title={`Connected to Odoo ${result.version}`}
    >
      <div className="space-y-2">
        <p className="text-xs opacity-80">{result.url}</p>
        <p>{passwordLine}</p>
        {result.list_error ? (
          <p>{result.list_error}</p>
        ) : (
          <div>
            <p className="mb-1.5">{result.databases.length} database(s):</p>
            <div className="flex flex-wrap gap-1">
              {result.databases.map((db) => (
                <Badge key={db} tone="gray">
                  {db}
                </Badge>
              ))}
            </div>
          </div>
        )}
      </div>
    </Alert>
  );
}

export function InstanceForm({
  instance,
  open,
  onClose,
}: {
  instance: Instance | null;
  open: boolean;
  onClose: () => void;
}) {
  const save = useSaveInstance();
  const test = useTestInstance();
  const toast = useToast();
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [verifyTls, setVerifyTls] = useState(true);
  const [timeout, setTimeoutSecs] = useState(3600);
  const [notes, setNotes] = useState("");

  useEffect(() => {
    if (!open) return;
    setName(instance?.name ?? "");
    setUrl(instance?.url ?? "");
    setPassword("");
    setShowPassword(false);
    setVerifyTls(instance?.verify_tls ?? true);
    setTimeoutSecs(instance?.timeout_secs ?? 3600);
    setNotes(instance?.notes ?? "");
    save.reset();
    test.reset();
  }, [open, instance?.id]);

  const runTest = () =>
    test.mutate({
      url,
      master_password: password || undefined,
      verify_tls: verifyTls,
      instance_id: instance?.id,
    });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      const saved = await save.mutateAsync({
        id: instance?.id,
        input: {
          name,
          url,
          master_password: password || undefined,
          verify_tls: verifyTls,
          timeout_secs: timeout,
          notes,
        },
      });
      toast.success(instance ? "Instance updated" : "Instance added", saved.odoo_version ? `Odoo ${saved.odoo_version}` : undefined);
      onClose();
    } catch {
      // shown inline
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      dismissable={false}
      title={instance ? `Edit ${instance.name}` : "Add an Odoo instance"}
      description="Backups use Odoo's database manager, exactly like the Backup button on /web/database/manager."
      size="lg"
      footer={
        <>
          <Button onClick={runTest} loading={test.isPending} icon={<PlugZap />} disabled={!url}>
            Test connection
          </Button>
          <div className="flex-1" />
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form="instance-form" variant="primary" loading={save.isPending}>
            {instance ? "Save" : "Add instance"}
          </Button>
        </>
      }
    >
      <form id="instance-form" onSubmit={submit} className="space-y-4">
        {save.error && <Alert tone="red">{errorMessage(save.error)}</Alert>}
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Name">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="ACME production" required />
          </Field>
          <Field label="URL" hint="The address you open Odoo with.">
            <Input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://erp.example.com"
              type="url"
              required
            />
          </Field>
        </div>
        <Field
          label="Master password"
          hint={
            instance?.has_master_password
              ? "Leave empty to keep the stored password."
              : "The admin_passwd of the Odoo server (database manager password). Stored encrypted."
          }
        >
          <div className="relative">
            <Input
              type={showPassword ? "text" : "password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={instance?.has_master_password ? "•••••••• (unchanged)" : ""}
              autoComplete="new-password"
              className="pr-10"
            />
            <IconButton
              label={showPassword ? "Hide" : "Show"}
              onClick={() => setShowPassword(!showPassword)}
              className="absolute right-1 top-1/2 -translate-y-1/2"
            >
              {showPassword ? <EyeOff /> : <Eye />}
            </IconButton>
          </div>
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Timeout (seconds)" hint="Max. wait for Odoo to prepare a backup before data starts flowing.">
            <Input
              type="number"
              min={60}
              max={86400}
              value={timeout}
              onChange={(e) => setTimeoutSecs(Number(e.target.value))}
            />
          </Field>
          <div className="pt-7">
            <Switch
              checked={verifyTls}
              onChange={setVerifyTls}
              label="Verify TLS certificate"
              description="Disable only for self-signed certificates."
            />
          </div>
        </div>
        <Field label="Notes">
          <Textarea value={notes} onChange={(e) => setNotes(e.target.value)} rows={2} placeholder="Optional" />
        </Field>
        {test.data && <TestResult result={test.data} />}
        {test.error && <Alert tone="red">{errorMessage(test.error)}</Alert>}
      </form>
    </Modal>
  );
}
