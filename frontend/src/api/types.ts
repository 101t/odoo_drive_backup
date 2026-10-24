export type RunStatus = "queued" | "running" | "success" | "partial" | "failed" | "cancelled";
export type ArtifactStatus = "pending" | "uploading" | "stored" | "failed" | "skipped" | "deleted";
export type BackupFormat = "zip" | "dump";

export interface User {
  id: number;
  username: string;
}

export interface AuthStatus {
  setup_required: boolean;
  user: User | null;
  version: string;
}

export interface Progress {
  run_id: number;
  phase: string;
  label: string | null;
  bytes_done: number;
  bytes_total: number | null;
  rate_bps: number;
}

export interface Artifact {
  id: number;
  run_id: number;
  destination_id: number | null;
  destination_name: string;
  destination_kind: "gdrive" | "local";
  status: ArtifactStatus;
  remote_id: string | null;
  remote_path: string | null;
  web_url: string | null;
  size_bytes: number | null;
  error: string | null;
  created_at: string;
  stored_at: string | null;
  deleted_at: string | null;
  delete_reason: string | null;
  /** Legal hold: never pruned by retention, cannot be deleted until unpinned. */
  pinned: boolean;
}

export interface Run {
  id: number;
  kind: "backup" | "restore";
  job_id: number | null;
  job_name: string | null;
  instance_id: number | null;
  instance_name: string;
  database: string;
  format: BackupFormat;
  trigger: "manual" | "schedule";
  status: RunStatus;
  phase: string | null;
  error: string | null;
  file_name: string | null;
  size_bytes: number | null;
  sha256: string | null;
  md5: string | null;
  odoo_version: string | null;
  source_artifact_id: number | null;
  restore_options: string | null;
  attempt: number;
  /** Set when the backup succeeded but its sanity check found something suspicious. */
  warning: string | null;
  /** JSON of the archive report: entries, filestore_files, dump_bytes, modules, pg_version... */
  manifest_json: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  progress: Progress | null;
  artifacts: Artifact[];
}

export interface RunLog {
  id: number;
  run_id: number;
  ts: string;
  level: "info" | "warn" | "error";
  message: string;
}

export interface RunDetail {
  run: Run;
  logs: RunLog[];
  restore_options: {
    copy: boolean;
    neutralize: boolean;
    source_run_id: number;
    source_instance: string;
    source_database: string;
  } | null;
}

export interface RunList {
  items: Run[];
  total: number;
}

export interface Instance {
  id: number;
  name: string;
  url: string;
  has_master_password: boolean;
  verify_tls: boolean;
  timeout_secs: number;
  notes: string;
  odoo_version: string | null;
  databases: string[];
  last_checked_at: string | null;
  last_check_error: string | null;
  created_at: string;
  updated_at: string;
  job_count: number;
  last_backup_at: string | null;
  last_backup_status: RunStatus | null;
}

export interface DatabaseDetail {
  name: string;
  exists: boolean;
  last_success_at: string | null;
  last_run_at: string | null;
  runs: number;
  last_status: RunStatus | null;
  last_size: number | null;
  jobs: { id: number; name: string; enabled: boolean }[];
}

export interface InstanceDetail extends Instance {
  database_details: DatabaseDetail[];
}

export interface InstanceInput {
  name: string;
  url: string;
  master_password?: string;
  verify_tls: boolean;
  timeout_secs: number;
  notes: string;
}

export interface InstanceTest {
  ok: boolean;
  url: string | null;
  version: string | null;
  databases: string[];
  list_error: string | null;
  master_password: "valid" | "invalid" | "unknown" | "missing";
  error: string | null;
}

export interface GdriveConfig {
  auth: "service_account" | "oauth";
  folder_id: string;
  subfolder_per_database: boolean;
  impersonate?: string | null;
  client_id?: string | null;
  service_account_email?: string | null;
  account_email?: string | null;
}

export interface LocalConfig {
  path: string;
  subfolder_per_database: boolean;
  /** The directory is on another site (network mount, removable disk taken away). */
  off_site?: boolean;
}

export interface Capabilities {
  hash: "md5" | "sha256";
  medium: "disk" | "cloud";
  off_site: boolean;
  immutable: boolean;
  can_delete: boolean;
}

export interface Manifest {
  entries: number;
  filestore_files: number;
  dump_bytes: number;
  odoo_version: string | null;
  db_name: string | null;
  pg_version: string | null;
  modules: number | null;
  crc_checked: boolean;
}

export type PolicyRuleId = "three_copies" | "two_media" | "one_off_site" | "one_immutable" | "zero_errors";

export interface PolicyScore {
  score: number;
  max: number;
  rules: { id: PolicyRuleId; ok: boolean }[];
}

export interface Destination {
  id: number;
  name: string;
  kind: "gdrive" | "local";
  config: GdriveConfig | LocalConfig;
  has_service_account_key: boolean;
  has_client_secret: boolean;
  connected: boolean;
  capabilities: Capabilities;
  stored_files: number;
  stored_bytes: number;
  job_count: number;
  created_at: string;
  updated_at: string;
}

export interface DestinationInput {
  name: string;
  kind: "gdrive" | "local";
  config: GdriveConfig | LocalConfig;
  secret?: {
    service_account_json?: string;
    client_secret?: string;
    refresh_token?: string;
  };
}

export interface TestStep {
  name: string;
  status: "ok" | "warn" | "fail";
  detail: string;
}

export interface TestReport {
  ok: boolean;
  steps: TestStep[];
}

export interface Job {
  id: number;
  name: string;
  instance_id: number;
  instance_name: string;
  all_databases: boolean;
  databases: string[];
  format: BackupFormat;
  cron: string;
  cron_description: string;
  timezone: string;
  enabled: boolean;
  keep_last: number;
  keep_daily: number;
  keep_weekly: number;
  keep_monthly: number;
  keep_yearly: number;
  retries: number;
  verify_archive: boolean;
  destination_ids: number[];
  next_run_at: string | null;
  last_run_at: string | null;
  last_status: RunStatus | null;
  policy: PolicyScore;
  created_at: string;
  updated_at: string;
}

export type JobInput = Omit<
  Job,
  | "id"
  | "instance_name"
  | "cron_description"
  | "next_run_at"
  | "last_run_at"
  | "last_status"
  | "policy"
  | "created_at"
  | "updated_at"
>;

export interface CronPreview {
  valid: boolean;
  description?: string;
  error?: string;
  next: string[];
}

export type ChannelKind = "webhook" | "telegram" | "email" | "heartbeat";

export interface Channel {
  id: number;
  name: string;
  kind: ChannelKind;
  config: Record<string, unknown>;
  has_secret: boolean;
  on_success: boolean;
  on_failure: boolean;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface ChannelInput {
  name: string;
  kind: ChannelKind;
  config: Record<string, unknown>;
  secret?: { url?: string; bot_token?: string; password?: string };
  on_success: boolean;
  on_failure: boolean;
  enabled: boolean;
}

export interface DatabaseHealth {
  instance_id: number;
  instance_name: string;
  database: string;
  last_status: RunStatus | null;
  last_success_at: string | null;
  last_run_at: string | null;
  last_size: number | null;
  last_warning: boolean;
  protected: boolean;
  stale: boolean;
}

export interface Dashboard {
  counts: {
    instances: number;
    databases: number;
    protected: number;
    stale: number;
    failing: number;
    jobs: number;
    jobs_enabled: number;
    destinations: number;
  };
  last24h: { success: number; partial: number; failed: number; bytes: number };
  storage: { id: number; name: string; kind: "gdrive" | "local"; files: number; bytes: number }[];
  activity: { day: string; success: number; failed: number; bytes: number }[];
  upcoming: { job_id: number; job_name: string; instance_name: string; next_run_at: string }[];
  health: DatabaseHealth[];
  active: Run[];
  recent: Run[];
  stale_after_hours: number;
}

export interface SettingsResponse {
  settings: { stale_after_hours: number; log_retention_days: number; size_drop_warn_percent: number };
  system: {
    version: string;
    data_dir: string;
    /** Where the application data lives: "SQLite" (file in the data directory) or "PostgreSQL". */
    database: { engine: string; version: string | null; size_bytes: number | null };
    temp_dir: string;
    temp_free_bytes: number | null;
    max_concurrent_jobs: number;
    public_url: string | null;
    metrics_enabled: boolean;
    started_at: string;
    active_runs: number;
  };
}
