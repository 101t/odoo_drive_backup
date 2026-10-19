import { Cloud, FileKey, HardDrive, Info, KeyRound, UserRound } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveDestination, useSettings, useStartOAuth } from "../api/hooks";
import type { Destination, DestinationInput, GdriveConfig, LocalConfig } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Button, CopyButton, Field, Input, Modal, Segmented, Switch, Textarea } from "../components/ui";

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
  const [path, setPath] = useState("/backups");
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    if (!open) return;
    save.reset();
    setName(destination?.name ?? "");
    setKind(destination?.kind ?? "gdrive");
    setSaJson("");
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
  }, [open, destination?.id]);

  const origin = settings?.system.public_url ?? window.location.origin;
  const redirectUri = `${origin}/api/oauth/google/callback`;
  const gdrive = destination?.kind === "gdrive" ? destination : null;
  const needsConnect = kind === "gdrive" && auth === "oauth" && !(gdrive?.connected && (gdrive.config as GdriveConfig).client_id === clientId);

  const loadKeyFile = async (file: File | undefined) => {
    if (!file) return;
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
        : { name, kind, config: { path, subfolder_per_database: subfolders } };
    try {
      const saved = await save.mutateAsync({ id: destination?.id, input });
      if (needsConnect && !saved.connected) {
        const { url } = await startOAuth.mutateAsync(saved.id);
        window.location.href = url;
        return;
      }
      toast.success(destination ? "Destination updated" : "Destination added", "Use “Test” to check it end to end.");
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
      title={destination ? `Edit ${destination.name}` : "Add a destination"}
      description="Where backups are stored. A job can copy each backup to several destinations."
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            type="submit"
            form="destination-form"
            variant="primary"
            loading={save.isPending || startOAuth.isPending}
            icon={needsConnect ? <KeyRound /> : undefined}
          >
            {needsConnect ? "Save & connect Google account" : destination ? "Save" : "Add destination"}
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
              { value: "gdrive", label: (<><Cloud /> Google Drive</>) },
              { value: "local", label: (<><HardDrive /> Local / NAS folder</>) },
            ]}
          />
        )}

        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} required placeholder={kind === "gdrive" ? "Google Drive" : "NAS"} />
        </Field>

        {kind === "gdrive" && (
          <>
            <div className="space-y-2">
              <p className="text-sm font-medium">Authentication</p>
              <Segmented
                value={auth}
                onChange={setAuth}
                options={[
                  { value: "service_account", label: (<><FileKey /> Service account</>) },
                  { value: "oauth", label: (<><UserRound /> Google account (OAuth)</>) },
                ]}
              />
            </div>

            {auth === "service_account" ? (
              <>
                <Alert tone="blue" icon={<Info />} title="Use a Shared Drive">
                  Service accounts have no storage quota of their own. Create a folder in a Shared Drive (Google
                  Workspace), add the service account e-mail as <b>Content manager</b>, and paste the folder link
                  below. With a personal Gmail account, use OAuth instead.
                </Alert>
                <Field
                  label="Service account key (JSON)"
                  hint={
                    serviceAccountEmail
                      ? `A key for ${serviceAccountEmail} is stored. Upload a new file only to replace it.`
                      : "Google Cloud console → IAM → Service accounts → Keys → Add key → JSON."
                  }
                >
                  <div className="space-y-2">
                    <input
                      type="file"
                      accept="application/json,.json"
                      onChange={(e) => loadKeyFile(e.target.files?.[0])}
                      className="block w-full text-sm text-zinc-600 file:mr-3 file:rounded-md file:border-0 file:bg-brand-50 file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-brand-700 hover:file:bg-brand-100 dark:text-zinc-400 dark:file:bg-brand-500/10 dark:file:text-brand-300"
                    />
                    <Textarea
                      value={saJson}
                      onChange={(e) => setSaJson(e.target.value)}
                      placeholder='…or paste the JSON: { "type": "service_account", … }'
                      rows={3}
                      className="font-mono text-xs"
                      spellCheck={false}
                    />
                  </div>
                </Field>
                <Field label="Folder" hint="Folder ID or full link, e.g. https://drive.google.com/drive/folders/1AbC…">
                  <Input value={folderId} onChange={(e) => setFolderId(e.target.value)} required={!impersonate} spellCheck={false} />
                </Field>
                <button
                  type="button"
                  className="text-xs font-medium text-brand-600 hover:underline dark:text-brand-400"
                  onClick={() => setShowAdvanced(!showAdvanced)}
                >
                  {showAdvanced ? "Hide" : "Show"} advanced options
                </button>
                {showAdvanced && (
                  <Field
                    label="Impersonate user (domain-wide delegation)"
                    hint="Workspace only: act as this user so files count against their quota. Requires delegation for the https://www.googleapis.com/auth/drive scope."
                  >
                    <Input type="email" value={impersonate} onChange={(e) => setImpersonate(e.target.value)} placeholder="backup@example.com" />
                  </Field>
                )}
              </>
            ) : (
              <>
                <Alert tone="blue" icon={<Info />} title="One-time setup in Google Cloud">
                  <ol className="ml-4 list-decimal space-y-1">
                    <li>Enable the Google Drive API and create an OAuth client of type <b>Web application</b>.</li>
                    <li>
                      Add this authorized redirect URI:
                      <span className="mt-1 flex items-center gap-1 rounded-md bg-white/60 px-2 py-1 font-mono text-xs dark:bg-black/20">
                        <span className="min-w-0 flex-1 truncate">{redirectUri}</span>
                        <CopyButton value={redirectUri} />
                      </span>
                    </li>
                    <li>
                      Publish the consent screen (<b>In production</b>): apps left in “Testing” get tokens that expire
                      after 7 days.
                    </li>
                  </ol>
                </Alert>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label="Client ID">
                    <Input value={clientId} onChange={(e) => setClientId(e.target.value)} required spellCheck={false} />
                  </Field>
                  <Field label="Client secret">
                    <Input
                      type="password"
                      value={clientSecret}
                      onChange={(e) => setClientSecret(e.target.value)}
                      placeholder={gdrive?.has_client_secret ? "•••••••• (unchanged)" : ""}
                      required={!gdrive?.has_client_secret}
                      autoComplete="off"
                    />
                  </Field>
                </div>
                <Field label="Folder (optional)" hint='Empty = a folder named "Odoo Backups" in My Drive is created automatically.'>
                  <Input value={folderId} onChange={(e) => setFolderId(e.target.value)} spellCheck={false} />
                </Field>
              </>
            )}
          </>
        )}

        {kind === "local" && (
          <Field
            label="Directory"
            hint="Absolute path on the server (in Docker: a mounted volume such as /backups). NFS/SMB mounts work too."
          >
            <Input value={path} onChange={(e) => setPath(e.target.value)} required spellCheck={false} className="font-mono" />
          </Field>
        )}

        <Switch
          checked={subfolders}
          onChange={setSubfolders}
          label="One folder per instance and database"
          description="Store files as <instance>/<database>/<file>.zip instead of all in one folder."
        />
      </form>
    </Modal>
  );
}
