import { Mail, Send, Webhook } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveChannel } from "../api/hooks";
import type { Channel, ChannelInput, ChannelKind } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Button, Field, Input, Modal, Segmented, Select, Switch } from "../components/ui";

export function NotificationForm({ channel, open, onClose }: { channel: Channel | null; open: boolean; onClose: () => void }) {
  const save = useSaveChannel();
  const toast = useToast();
  const [kind, setKind] = useState<ChannelKind>("webhook");
  const [name, setName] = useState("");
  const [onFailure, setOnFailure] = useState(true);
  const [onSuccess, setOnSuccess] = useState(false);
  const [enabled, setEnabled] = useState(true);
  // webhook
  const [format, setFormat] = useState("slack");
  const [url, setUrl] = useState("");
  // telegram
  const [chatId, setChatId] = useState("");
  const [botToken, setBotToken] = useState("");
  // email
  const [host, setHost] = useState("");
  const [port, setPort] = useState(587);
  const [security, setSecurity] = useState("starttls");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

  useEffect(() => {
    if (!open) return;
    save.reset();
    const c = (channel?.config ?? {}) as Record<string, unknown>;
    setKind(channel?.kind ?? "webhook");
    setName(channel?.name ?? "");
    setOnFailure(channel?.on_failure ?? true);
    setOnSuccess(channel?.on_success ?? false);
    setEnabled(channel?.enabled ?? true);
    setFormat((c.format as string) ?? "slack");
    setUrl("");
    setChatId((c.chat_id as string) ?? "");
    setBotToken("");
    setHost((c.host as string) ?? "");
    setPort((c.port as number) ?? 587);
    setSecurity((c.security as string) ?? "starttls");
    setUsername((c.username as string) ?? "");
    setPassword("");
    setFrom((c.from as string) ?? "");
    setTo(((c.to as string[]) ?? []).join(", "));
  }, [open, channel?.id]);

  const secretKept = channel?.has_secret ? "•••••••• (unchanged)" : "";

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    let input: ChannelInput;
    const common = { name, kind, on_failure: onFailure, on_success: onSuccess, enabled };
    if (kind === "webhook") {
      input = { ...common, config: { format }, secret: { url: url || undefined } };
    } else if (kind === "telegram") {
      input = { ...common, config: { chat_id: chatId }, secret: { bot_token: botToken || undefined } };
    } else {
      input = {
        ...common,
        config: {
          host,
          port,
          security,
          username: username || null,
          from,
          to: to.split(/[,;\s]+/).filter(Boolean),
        },
        secret: { password: password || undefined },
      };
    }
    try {
      await save.mutateAsync({ id: channel?.id, input });
      toast.success(channel ? "Channel updated" : "Channel added", "Use “Test” to send a test message.");
      onClose();
    } catch {
      // inline
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      dismissable={false}
      title={channel ? `Edit ${channel.name}` : "Add a notification channel"}
      description="Be told when a backup fails, so problems never go unnoticed."
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form="channel-form" variant="primary" loading={save.isPending}>
            {channel ? "Save" : "Add channel"}
          </Button>
        </>
      }
    >
      <form id="channel-form" onSubmit={submit} className="space-y-4">
        {save.error && <Alert tone="red">{errorMessage(save.error)}</Alert>}
        {!channel && (
          <Segmented
            value={kind}
            onChange={setKind}
            options={[
              { value: "webhook", label: (<><Webhook /> Webhook</>) },
              { value: "telegram", label: (<><Send /> Telegram</>) },
              { value: "email", label: (<><Mail /> E-mail</>) },
            ]}
          />
        )}
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} required placeholder="Ops alerts" />
        </Field>

        {kind === "webhook" && (
          <>
            <Field label="Payload format">
              <Select value={format} onChange={(e) => setFormat(e.target.value)}>
                <option value="slack">Slack / Mattermost / Rocket.Chat / Google Chat</option>
                <option value="discord">Discord</option>
                <option value="generic">Generic JSON (event, text, run details)</option>
              </Select>
            </Field>
            <Field label="Webhook URL" hint="Stored encrypted.">
              <Input type="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder={secretKept || "https://hooks.slack.com/services/…"} required={!channel?.has_secret} />
            </Field>
          </>
        )}

        {kind === "telegram" && (
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Bot token" hint="From @BotFather. Stored encrypted.">
              <Input type="password" value={botToken} onChange={(e) => setBotToken(e.target.value)} placeholder={secretKept} required={!channel?.has_secret} autoComplete="off" />
            </Field>
            <Field label="Chat ID" hint="User, group or channel ID (e.g. -1001234567890).">
              <Input value={chatId} onChange={(e) => setChatId(e.target.value)} required />
            </Field>
          </div>
        )}

        {kind === "email" && (
          <>
            <div className="grid gap-4 sm:grid-cols-3">
              <Field label="SMTP server" className="sm:col-span-2">
                <Input value={host} onChange={(e) => setHost(e.target.value)} placeholder="smtp.example.com" required />
              </Field>
              <Field label="Port">
                <Input type="number" value={port} onChange={(e) => setPort(Number(e.target.value))} required />
              </Field>
            </div>
            <Field label="Security">
              <Select
                value={security}
                onChange={(e) => {
                  setSecurity(e.target.value);
                  setPort(e.target.value === "tls" ? 465 : e.target.value === "starttls" ? 587 : 25);
                }}
              >
                <option value="starttls">STARTTLS (port 587)</option>
                <option value="tls">TLS (port 465)</option>
                <option value="none">None (local relay only)</option>
              </Select>
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Username">
                <Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" />
              </Field>
              <Field label="Password">
                <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={secretKept} autoComplete="new-password" />
              </Field>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="From">
                <Input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="Odoo Backup <backup@example.com>" required />
              </Field>
              <Field label="To" hint="Comma-separated addresses.">
                <Input value={to} onChange={(e) => setTo(e.target.value)} placeholder="ops@example.com" required />
              </Field>
            </div>
          </>
        )}

        <div className="space-y-3 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <Switch checked={onFailure} onChange={setOnFailure} label="On failure" description="Failed or partially failed runs." />
          <Switch checked={onSuccess} onChange={setOnSuccess} label="On success" description="Every successful run (can be noisy)." />
          <Switch checked={enabled} onChange={setEnabled} label="Enabled" />
        </div>
      </form>
    </Modal>
  );
}
