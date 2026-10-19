import { CircleCheck, CircleX, Eye, EyeOff, PlugZap, TriangleAlert } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveInstance, useTestInstance } from "../api/hooks";
import type { Instance, InstanceTest } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Badge, Button, Field, IconButton, Input, Modal, Switch, Textarea } from "../components/ui";
import { ts, useT } from "../i18n";

function TestResult({ result }: { result: InstanceTest }) {
  const t = useT();
  if (result.error) {
    return (
      <Alert tone="red" icon={<CircleX />} title={t("instance_form.test_failed")}>
        {ts(result.error)}
      </Alert>
    );
  }
  const passwordLine = {
    valid: <span className="text-emerald-700 dark:text-emerald-400">{t("instance_form.password_valid")}</span>,
    invalid: (
      <span className="font-medium text-rose-700 dark:text-rose-400">{t("instance_form.password_invalid")}</span>
    ),
    unknown: <span>{t("instance_form.password_unknown")}</span>,
    missing: <span>{t("instance_form.password_missing")}</span>,
  }[result.master_password];
  return (
    <Alert
      tone={result.ok && !result.list_error ? "green" : "amber"}
      icon={result.ok ? <CircleCheck /> : <TriangleAlert />}
      title={t("instance_form.connected", { version: result.version ?? "" })}
    >
      <div className="space-y-2">
        <p className="text-xs opacity-80" dir="ltr">
          {result.url}
        </p>
        <p>{passwordLine}</p>
        {result.list_error ? (
          <p>{ts(result.list_error)}</p>
        ) : (
          <div>
            <p className="mb-1.5">{t("instance_form.databases_found", { count: result.databases.length })}</p>
            <div className="flex flex-wrap gap-1">
              {result.databases.map((db) => (
                <Badge key={db} tone="gray">
                  <bdi>{db}</bdi>
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
  const t = useT();
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
      toast.success(
        instance ? t("instance_form.updated") : t("instance_form.added"),
        saved.odoo_version ? `Odoo ${saved.odoo_version}` : undefined,
      );
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
      title={instance ? t("instance_form.edit_title", { name: instance.name }) : t("instance_form.add_title")}
      description={t("instance_form.description")}
      size="lg"
      footer={
        <>
          <Button onClick={runTest} loading={test.isPending} icon={<PlugZap />} disabled={!url}>
            {t("instance_form.test")}
          </Button>
          <div className="flex-1" />
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button type="submit" form="instance-form" variant="primary" loading={save.isPending}>
            {instance ? t("common.save") : t("instances.add")}
          </Button>
        </>
      }
    >
      <form id="instance-form" onSubmit={submit} className="space-y-4">
        {save.error && <Alert tone="red">{errorMessage(save.error)}</Alert>}
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label={t("common.name")}>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("instance_form.name_placeholder")}
              dir="auto"
              required
            />
          </Field>
          <Field label={t("instance_form.url")} hint={t("instance_form.url_hint")}>
            <Input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://erp.example.com"
              type="url"
              dir="ltr"
              required
            />
          </Field>
        </div>
        <Field
          label={t("instance_form.master_password")}
          hint={instance?.has_master_password ? t("instance_form.password_keep") : t("instance_form.password_hint")}
        >
          <div className="relative">
            <Input
              type={showPassword ? "text" : "password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={instance?.has_master_password ? t("instance_form.password_unchanged") : ""}
              autoComplete="new-password"
              className="pe-10"
            />
            <IconButton
              label={showPassword ? t("common.hide") : t("common.show")}
              onClick={() => setShowPassword(!showPassword)}
              className="absolute end-1 top-1/2 -translate-y-1/2"
            >
              {showPassword ? <EyeOff /> : <Eye />}
            </IconButton>
          </div>
        </Field>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label={t("instance_form.timeout")} hint={t("instance_form.timeout_hint")}>
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
              label={t("instance_form.verify_tls")}
              description={t("instance_form.verify_tls_hint")}
            />
          </div>
        </div>
        <Field label={t("common.notes")}>
          <Textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            rows={2}
            placeholder={t("common.optional")}
            dir="auto"
          />
        </Field>
        {test.data && <TestResult result={test.data} />}
        {test.error && <Alert tone="red">{errorMessage(test.error)}</Alert>}
      </form>
    </Modal>
  );
}
