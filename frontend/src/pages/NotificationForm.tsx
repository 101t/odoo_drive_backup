import { HeartPulse, Mail, Send, Webhook } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api/client";
import { useSaveChannel } from "../api/hooks";
import type { Channel, ChannelInput, ChannelKind } from "../api/types";
import { useToast } from "../components/toast";
import { Alert, Button, Field, Input, Modal, Segmented, Select, Switch } from "../components/ui";
import { useT } from "../i18n";

export function NotificationForm({ channel, open, onClose }: { channel: Channel | null; open: boolean; onClose: () => void }) {
  const t = useT();
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
  // heartbeat
  const [style, setStyle] = useState("healthchecks");

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
    setStyle((c.style as string) ?? "healthchecks");
  }, [open, channel?.id]);

  const chooseKind = (next: ChannelKind) => {
    setKind(next);
    // A heartbeat is a ping on every successful run: the monitor alerts when pings stop.
    if (next === "heartbeat") setOnSuccess(true);
  };

  const secretKept = channel?.has_secret ? t("instance_form.password_unchanged") : "";

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    let input: ChannelInput;
    const common = { name, kind, on_failure: onFailure, on_success: onSuccess, enabled };
    if (kind === "webhook") {
      input = { ...common, config: { format }, secret: { url: url || undefined } };
    } else if (kind === "telegram") {
      input = { ...common, config: { chat_id: chatId }, secret: { bot_token: botToken || undefined } };
    } else if (kind === "heartbeat") {
      input = { ...common, config: { style }, secret: { url: url || undefined } };
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
      toast.success(channel ? t("notification_form.updated") : t("notification_form.added"), t("notification_form.test_hint"));
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
      title={channel ? t("notification_form.edit_title", { name: channel.name }) : t("notification_form.add_title")}
      description={t("notification_form.description")}
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button type="submit" form="channel-form" variant="primary" loading={save.isPending}>
            {channel ? t("common.save") : t("notifications.add")}
          </Button>
        </>
      }
    >
      <form id="channel-form" onSubmit={submit} className="space-y-4">
        {save.error && <Alert tone="red">{errorMessage(save.error)}</Alert>}
        {!channel && (
          <Segmented
            value={kind}
            onChange={chooseKind}
            options={[
              {
                value: "webhook",
                label: (
                  <>
                    <Webhook /> {t("notification_form.webhook")}
                  </>
                ),
              },
              {
                value: "telegram",
                label: (
                  <>
                    <Send /> {t("notification_form.telegram")}
                  </>
                ),
              },
              {
                value: "email",
                label: (
                  <>
                    <Mail /> {t("notification_form.email")}
                  </>
                ),
              },
              {
                value: "heartbeat",
                label: (
                  <>
                    <HeartPulse /> {t("notification_form.heartbeat")}
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
            placeholder={t("notification_form.name_placeholder")}
          />
        </Field>

        {kind === "webhook" && (
          <>
            <Field label={t("notification_form.payload")}>
              <Select value={format} onChange={(e) => setFormat(e.target.value)}>
                <option value="slack">{t("notification_form.payload_slack")}</option>
                <option value="discord">{t("notification_form.payload_discord")}</option>
                <option value="generic">{t("notification_form.payload_generic")}</option>
              </Select>
            </Field>
            <Field label={t("notification_form.webhook_url")} hint={t("notification_form.stored_encrypted")}>
              <Input
                type="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder={secretKept || "https://hooks.slack.com/services/…"}
                required={!channel?.has_secret}
                dir="ltr"
              />
            </Field>
          </>
        )}

        {kind === "heartbeat" && (
          <>
            <Alert tone="blue">{t("notification_form.heartbeat_hint")}</Alert>
            <Field label={t("notification_form.heartbeat_style")}>
              <Select value={style} onChange={(e) => setStyle(e.target.value)}>
                <option value="healthchecks">{t("notification_form.heartbeat_healthchecks")}</option>
                <option value="uptime_kuma">{t("notification_form.heartbeat_uptime_kuma")}</option>
                <option value="plain">{t("notification_form.heartbeat_plain")}</option>
              </Select>
            </Field>
            <Field label={t("notification_form.ping_url")} hint={t("notification_form.stored_encrypted")}>
              <Input
                type="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder={secretKept || "https://hc-ping.com/…"}
                required={!channel?.has_secret}
                dir="ltr"
              />
            </Field>
          </>
        )}

        {kind === "telegram" && (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field label={t("notification_form.bot_token")} hint={t("notification_form.bot_token_hint")}>
              <Input
                type="password"
                value={botToken}
                onChange={(e) => setBotToken(e.target.value)}
                placeholder={secretKept}
                required={!channel?.has_secret}
                autoComplete="off"
              />
            </Field>
            <Field label={t("notification_form.chat_id")} hint={t("notification_form.chat_id_hint")}>
              <Input value={chatId} onChange={(e) => setChatId(e.target.value)} required dir="ltr" />
            </Field>
          </div>
        )}

        {kind === "email" && (
          <>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
              <Field label={t("notification_form.smtp_server")} className="sm:col-span-2">
                <Input value={host} onChange={(e) => setHost(e.target.value)} placeholder="smtp.example.com" required dir="ltr" />
              </Field>
              <Field label={t("notification_form.port")}>
                <Input type="number" value={port} onChange={(e) => setPort(Number(e.target.value))} required />
              </Field>
            </div>
            <Field label={t("notification_form.security")}>
              <Select
                value={security}
                onChange={(e) => {
                  setSecurity(e.target.value);
                  setPort(e.target.value === "tls" ? 465 : e.target.value === "starttls" ? 587 : 25);
                }}
              >
                <option value="starttls">{t("notification_form.security_starttls")}</option>
                <option value="tls">{t("notification_form.security_tls")}</option>
                <option value="none">{t("notification_form.security_none")}</option>
              </Select>
            </Field>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <Field label={t("login.username")}>
                <Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" dir="ltr" />
              </Field>
              <Field label={t("login.password")}>
                <Input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={secretKept}
                  autoComplete="new-password"
                />
              </Field>
            </div>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <Field label={t("notification_form.from")}>
                <Input
                  value={from}
                  onChange={(e) => setFrom(e.target.value)}
                  placeholder="Odoo Backup <backup@example.com>"
                  required
                  dir="ltr"
                />
              </Field>
              <Field label={t("notification_form.to")} hint={t("notification_form.to_hint")}>
                <Input value={to} onChange={(e) => setTo(e.target.value)} placeholder="ops@example.com" required dir="ltr" />
              </Field>
            </div>
          </>
        )}

        <div className="space-y-3 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <Switch
            checked={onFailure}
            onChange={setOnFailure}
            label={t("notification_form.on_failure")}
            description={t("notification_form.on_failure_hint")}
          />
          <Switch
            checked={onSuccess}
            onChange={setOnSuccess}
            label={t("notification_form.on_success")}
            description={t("notification_form.on_success_hint")}
          />
          <Switch checked={enabled} onChange={setEnabled} label={t("job_form.enabled")} />
        </div>
      </form>
    </Modal>
  );
}
