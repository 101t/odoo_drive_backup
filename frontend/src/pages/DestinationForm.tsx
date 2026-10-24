import { Cloud, FileKey, HardDrive, Info, KeyRound, Upload, UserRound } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveDestination, useSettings, useStartOAuth } from "../api/hooks";
import type { Destination, DestinationInput, GdriveConfig, LocalConfig } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Button, CopyButton, Field, Input, Modal, Segmented, Switch, Textarea } from "../components/ui";
import { tr, useT } from "../i18n";

type Kind = "gdrive" | "local";
type Auth = "service_account" | "oauth";

export function DestinationForm({
  destination,
  open,
  onClose,
}: {
  destination: Destination | null;
  open: boolean;
  onClose: () => void;
}) {
  const t = useT();
  const save = useSaveDestination();
  const startOAuth = useStartOAuth();
  const { data: settings } = useSettings();
  const toast = useToast();

  const [name, setName] = useState("");
  const [kind, setKind] = useState<Kind>("gdrive");
  const [auth, setAuth] = useState<Auth>("service_account");
  const [folderId, setFolderId] = useState("");
  const [subfolders, setSubfolders] = useState(true);
  const [impersonate, setImpersonate] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [saJson, setSaJson] = useState("");
  const [keyFileName, setKeyFileName] = useState("");
  const keyFileInput = useRef<HTMLInputElement>(null);
  const [path, setPath] = useState("/backups");
  const [offSite, setOffSite] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    if (!open) return;
    save.reset();
    setName(destination?.name ?? "");
    setKind(destination?.kind ?? "gdrive");
    setSaJson("");
    setKeyFileName("");
    setClientSecret("");
    if (destination?.kind === "gdrive") {
      const c = destination.config as GdriveConfig;
      setAuth(c.auth);
      setFolderId(c.folder_id ?? "");
      setSubfolders(c.subfolder_per_database);
      setImpersonate(c.impersonate ?? "");
      setClientId(c.client_id ?? "");
      setShowAdvanced(Boolean(c.impersonate));
    } else {
      setAuth("service_account");
      setFolderId("");
      setImpersonate("");
      setClientId("");
      setShowAdvanced(false);
      setSubfolders(destination ? (destination.config as LocalConfig).subfolder_per_database : true);
    }
    setPath(destination?.kind === "local" ? (destination.config as LocalConfig).path : "/backups");
    setOffSite(destination?.kind === "local" ? Boolean((destination.config as LocalConfig).off_site) : false);
  }, [open, destination?.id]);

  const origin = settings?.system.public_url ?? window.location.origin;
  const redirectUri = `${origin}/api/oauth/google/callback`;
  const gdrive = destination?.kind === "gdrive" ? destination : null;
  const needsConnect =
    kind === "gdrive" && auth === "oauth" && !(gdrive?.connected && (gdrive.config as GdriveConfig).client_id === clientId);

  const loadKeyFile = async (file: File | undefined) => {
    if (!file) return;
    setKeyFileName(file.name);
    const text = await file.text();
    setSaJson(text);
    try {
      const parsed = JSON.parse(text) as { client_email?: string };
      if (parsed.client_email && !name) setName(`Google Drive (${parsed.client_email.split("@")[0]})`);
    } catch {
      // validated by the server
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const input: DestinationInput =
      kind === "gdrive"
        ? {
            name,
            kind,
            config: {
              auth,
              folder_id: folderId,
              subfolder_per_database: subfolders,
              impersonate: auth === "service_account" ? impersonate || null : null,
              client_id: auth === "oauth" ? clientId : null,
            },
            secret: {
              service_account_json: auth === "service_account" ? saJson || undefined : undefined,
              client_secret: auth === "oauth" ? clientSecret || undefined : undefined,
            },
          }
        : { name, kind, config: { path, subfolder_per_database: subfolders, off_site: offSite } };
    try {
      const saved = await save.mutateAsync({ id: destination?.id, input });
      if (needsConnect && !saved.connected) {
        const { url } = await startOAuth.mutateAsync(saved.id);
        window.location.href = url;
        return;
      }
      toast.success(destination ? t("destination_form.updated") : t("destination_form.added"), t("destination_form.test_hint"));
      onClose();
    } catch {
      // shown inline
    }
  };

  const serviceAccountEmail = gdrive && (gdrive.config as GdriveConfig).service_account_email;

  return (
    <Modal
      open={open}
      onClose={onClose}
      dismissable={false}
      size="lg"
      title={destination ? t("destination_form.edit_title", { name: destination.name }) : t("destination_form.add_title")}
      description={t("destination_form.description")}
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            type="submit"
            form="destination-form"
            variant="primary"
            loading={save.isPending || startOAuth.isPending}
            icon={needsConnect ? <KeyRound /> : undefined}
          >
            {needsConnect ? t("destination_form.save_connect") : destination ? t("common.save") : t("destinations.add")}
          </Button>
        </>
      }
    >
      <form id="destination-form" onSubmit={submit} className="space-y-5">
        {(save.error || startOAuth.error) && <Alert tone="red">{errorMessage(save.error ?? startOAuth.error)}</Alert>}

        {!destination && (
          <Segmented
            value={kind}
            onChange={setKind}
            options={[
              {
                value: "gdrive",
                label: (
                  <>
                    <Cloud /> {t("destinations.google_drive")}
                  </>
                ),
              },
              {
                value: "local",
                label: (
                  <>
                    <HardDrive /> {t("destination_form.local")}
                  </>
                ),
              },
            ]}
          />
        )}

        <Field label={t("common.name")}>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            dir="auto"
            placeholder={kind === "gdrive" ? t("destinations.google_drive") : t("destination_form.nas")}
          />
        </Field>

        {kind === "gdrive" && (
          <>
            <div className="space-y-2">
              <p className="text-sm font-medium">{t("destination_form.authentication")}</p>
              <Segmented
                value={auth}
                onChange={setAuth}
                options={[
                  {
                    value: "service_account",
                    label: (
                      <>
                        <FileKey /> {t("destinations.service_account")}
                      </>
                    ),
                  },
                  {
                    value: "oauth",
                    label: (
                      <>
                        <UserRound /> {t("destination_form.google_account")}
                      </>
                    ),
                  },
                ]}
              />
            </div>

            {auth === "service_account" ? (
              <>
                <Alert tone="blue" icon={<Info />} title={t("destination_form.shared_drive_title")}>
                  {tr("destination_form.shared_drive_hint", { role: <b>{t("destination_form.content_manager")}</b> })}
                </Alert>
                <Field
                  label={t("destination_form.key")}
                  hint={
                    serviceAccountEmail
                      ? t("destination_form.key_stored", { email: serviceAccountEmail })
                      : t("destination_form.key_hint")
                  }
                >
                  <div className="space-y-2">
                    {/* The native file input cannot be translated: a button opens it instead. */}
                    <input
                      ref={keyFileInput}
                      type="file"
                      accept="application/json,.json"
                      onChange={(e) => loadKeyFile(e.target.files?.[0])}
                      className="hidden"
                    />
                    <div className="flex min-w-0 items-center gap-3">
                      <Button size="sm" icon={<Upload />} onClick={() => keyFileInput.current?.click()}>
                        {t("destination_form.choose_file")}
                      </Button>
                      <span className="truncate text-sm text-zinc-500">
                        {keyFileName ? <bdi>{keyFileName}</bdi> : t("destination_form.no_file")}
                      </span>
                    </div>
                    <Textarea
                      value={saJson}
                      onChange={(e) => setSaJson(e.target.value)}
                      placeholder={t("destination_form.key_paste")}
                      rows={3}
                      className="font-mono text-xs"
                      dir="auto"
                      spellCheck={false}
                    />
                  </div>
                </Field>
                <Field label={t("destinations.folder")} hint={t("destination_form.folder_hint")}>
                  <Input
                    value={folderId}
                    onChange={(e) => setFolderId(e.target.value)}
                    required={!impersonate}
                    dir="ltr"
                    spellCheck={false}
                  />
                </Field>
                <button
                  type="button"
                  className="text-xs font-medium text-brand-600 hover:underline dark:text-brand-400"
                  onClick={() => setShowAdvanced(!showAdvanced)}
                >
                  {showAdvanced ? t("destination_form.hide_advanced") : t("destination_form.show_advanced")}
                </button>
                {showAdvanced && (
                  <Field label={t("destination_form.impersonate")} hint={t("destination_form.impersonate_hint")}>
                    <Input
                      type="email"
                      value={impersonate}
                      onChange={(e) => setImpersonate(e.target.value)}
                      placeholder="backup@example.com"
                      dir="ltr"
                    />
                  </Field>
                )}
              </>
            ) : (
              <>
                <Alert tone="blue" icon={<Info />} title={t("destination_form.oauth_setup")}>
                  <ol className="ms-4 list-decimal space-y-1">
                    <li>{tr("destination_form.oauth_step1", { type: <b>{t("destination_form.web_application")}</b> })}</li>
                    <li>
                      {t("destination_form.oauth_step2")}
                      <span
                        dir="ltr"
                        className="mt-1 flex items-center gap-1 rounded-md bg-white/60 px-2 py-1 font-mono text-xs dark:bg-black/20"
                      >
                        <span className="min-w-0 flex-1 truncate">{redirectUri}</span>
                        <CopyButton value={redirectUri} />
                      </span>
                    </li>
                    <li>{tr("destination_form.oauth_step3", { status: <b>{t("destination_form.in_production")}</b> })}</li>
                  </ol>
                </Alert>
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <Field label={t("destination_form.client_id")}>
                    <Input
                      value={clientId}
                      onChange={(e) => setClientId(e.target.value)}
                      required
                      dir="ltr"
                      spellCheck={false}
                    />
                  </Field>
                  <Field label={t("destination_form.client_secret")}>
                    <Input
                      type="password"
                      value={clientSecret}
                      onChange={(e) => setClientSecret(e.target.value)}
                      placeholder={gdrive?.has_client_secret ? t("instance_form.password_unchanged") : ""}
                      required={!gdrive?.has_client_secret}
                      autoComplete="off"
                    />
                  </Field>
                </div>
                <Field label={t("destination_form.folder_optional")} hint={t("destination_form.folder_optional_hint")}>
                  <Input value={folderId} onChange={(e) => setFolderId(e.target.value)} dir="ltr" spellCheck={false} />
                </Field>
              </>
            )}
          </>
        )}

        {kind === "local" && (
          <Field label={t("destination_form.directory")} hint={t("destination_form.directory_hint")}>
            <Input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              required
              spellCheck={false}
              dir="ltr"
              className="font-mono"
            />
          </Field>
        )}

        {kind === "local" && (
          <Switch
            checked={offSite}
            onChange={setOffSite}
            label={t("destination_form.off_site")}
            description={t("destination_form.off_site_hint")}
          />
        )}

        <Switch
          checked={subfolders}
          onChange={setSubfolders}
          label={t("destination_form.subfolders")}
          description={t("destination_form.subfolders_hint")}
        />
      </form>
    </Modal>
  );
}
