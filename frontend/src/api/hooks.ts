import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocale } from "../i18n";
import { api } from "./client";
import type {
  AuthStatus,
  Channel,
  ChannelInput,
  CronPreview,
  Dashboard,
  Destination,
  DestinationInput,
  Instance,
  InstanceDetail,
  InstanceInput,
  InstanceTest,
  Job,
  JobInput,
  RunDetail,
  RunList,
  SettingsResponse,
  TestReport,
} from "./types";

export const keys = {
  auth: ["auth"] as const,
  dashboard: ["dashboard"] as const,
  instances: ["instances"] as const,
  instance: (id: number) => ["instances", id] as const,
  destinations: ["destinations"] as const,
  jobs: ["jobs"] as const,
  runs: ["runs"] as const,
  run: (id: number) => ["run", id] as const,
  channels: ["channels"] as const,
  settings: ["settings"] as const,
};

// ---------------------------------------------------------------- auth

export function useAuth() {
  return useQuery({ queryKey: keys.auth, queryFn: () => api.get<AuthStatus>("/api/auth/status"), staleTime: 60_000 });
}

export function useLogin(setup: boolean) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: { username: string; password: string }) =>
      api.post(setup ? "/api/auth/setup" : "/api/auth/login", body),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.auth }),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.post("/api/auth/logout"),
    onSuccess: () => {
      qc.clear();
      qc.invalidateQueries({ queryKey: keys.auth });
    },
  });
}

export function useChangePassword() {
  return useMutation({
    mutationFn: (body: { current_password: string; new_password: string }) => api.post("/api/auth/password", body),
  });
}

// ---------------------------------------------------------------- dashboard

export function useDashboard() {
  return useQuery({
    queryKey: keys.dashboard,
    queryFn: () => api.get<Dashboard>("/api/dashboard"),
    refetchInterval: 60_000,
  });
}

// ---------------------------------------------------------------- instances

export function useInstances() {
  return useQuery({ queryKey: keys.instances, queryFn: () => api.get<Instance[]>("/api/instances") });
}

export function useInstance(id: number) {
  return useQuery({
    queryKey: keys.instance(id),
    queryFn: () => api.get<InstanceDetail>(`/api/instances/${id}`),
  });
}

export function useSaveInstance() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id?: number; input: InstanceInput }) =>
      id ? api.put<Instance>(`/api/instances/${id}`, input) : api.post<Instance>("/api/instances", input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.instances });
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}

export function useDeleteInstance() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete(`/api/instances/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.instances });
      qc.invalidateQueries({ queryKey: keys.jobs });
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}

export function useTestInstance() {
  return useMutation({
    mutationFn: (body: { url: string; master_password?: string; verify_tls: boolean; instance_id?: number }) =>
      api.post<InstanceTest>("/api/instances/test", body),
  });
}

export function useRefreshInstance() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.post<{ instance: Instance; test: InstanceTest }>(`/api/instances/${id}/refresh`),
    onSuccess: (_, id) => {
      qc.invalidateQueries({ queryKey: keys.instances });
      qc.invalidateQueries({ queryKey: keys.instance(id) });
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}

// ---------------------------------------------------------------- destinations

export function useDestinations() {
  return useQuery({ queryKey: keys.destinations, queryFn: () => api.get<Destination[]>("/api/destinations") });
}

export function useSaveDestination() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id?: number; input: DestinationInput }) =>
      id ? api.put<Destination>(`/api/destinations/${id}`, input) : api.post<Destination>("/api/destinations", input),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.destinations }),
  });
}

export function useDeleteDestination() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete(`/api/destinations/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.destinations }),
  });
}

export function useTestDestination() {
  return useMutation({ mutationFn: (id: number) => api.post<TestReport>(`/api/destinations/${id}/test`) });
}

export function useStartOAuth() {
  return useMutation({
    mutationFn: (id: number) => api.post<{ url: string; redirect_uri: string }>(`/api/destinations/${id}/oauth/start`),
  });
}

// ---------------------------------------------------------------- jobs

export function useJobs() {
  return useQuery({ queryKey: keys.jobs, queryFn: () => api.get<Job[]>("/api/jobs") });
}

export function useSaveJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id?: number; input: JobInput }) =>
      id ? api.put<Job>(`/api/jobs/${id}`, input) : api.post<Job>("/api/jobs", input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.jobs });
      qc.invalidateQueries({ queryKey: keys.dashboard });
      qc.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useDeleteJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete(`/api/jobs/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.jobs });
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}

export function useRunJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.post<{ run_ids: number[] }>(`/api/jobs/${id}/run`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.runs });
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}

export function useCronPreview(cron: string, timezone: string) {
  // The description is composed by the server in the UI language.
  const locale = useLocale();
  return useQuery({
    queryKey: ["cron", cron, timezone, locale],
    queryFn: () => api.post<CronPreview>("/api/cron/preview", { cron, timezone }),
    enabled: cron.trim().length > 0,
    staleTime: 30_000,
    placeholderData: keepPreviousData,
  });
}

// ---------------------------------------------------------------- runs

export interface RunFilters {
  status?: string;
  instance_id?: number;
  job_id?: number;
  kind?: string;
  search?: string;
  limit?: number;
  offset?: number;
}

export function useRuns(filters: RunFilters) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) {
    if (value !== undefined && value !== "" && value !== null) params.set(key, String(value));
  }
  return useQuery({
    queryKey: [...keys.runs, filters],
    queryFn: () => api.get<RunList>(`/api/runs?${params}`),
    placeholderData: keepPreviousData,
  });
}

export function useRun(id: number) {
  return useQuery({ queryKey: keys.run(id), queryFn: () => api.get<RunDetail>(`/api/runs/${id}`) });
}

function useRunMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.runs });
      qc.invalidateQueries({ queryKey: ["run"] });
      qc.invalidateQueries({ queryKey: keys.dashboard });
      qc.invalidateQueries({ queryKey: keys.destinations });
    },
  });
}

export const useCancelRun = () => useRunMutation((id: number) => api.post(`/api/runs/${id}/cancel`));

export const useDeleteRun = () =>
  useRunMutation(({ id, deleteFiles }: { id: number; deleteFiles: boolean }) =>
    api.delete(`/api/runs/${id}?delete_files=${deleteFiles ? 1 : 0}`),
  );

export const useDeleteArtifact = () => useRunMutation((id: number) => api.delete(`/api/artifacts/${id}`));

export const usePinArtifact = () =>
  useRunMutation(({ id, pinned }: { id: number; pinned: boolean }) => api.put(`/api/artifacts/${id}/pin`, { pinned }));

export const useBackupNow = () =>
  useRunMutation((body: { instance_id: number; databases: string[]; destination_ids: number[]; format: string }) =>
    api.post<{ run_ids: number[] }>("/api/backups", body),
  );

export const useRestore = () =>
  useRunMutation(
    ({ artifactId, ...body }: { artifactId: number; instance_id: number; database: string; copy: boolean; neutralize: boolean }) =>
      api.post<{ run_id: number }>(`/api/artifacts/${artifactId}/restore`, body),
  );

// ---------------------------------------------------------------- notifications & settings

export function useChannels() {
  return useQuery({ queryKey: keys.channels, queryFn: () => api.get<Channel[]>("/api/notifications") });
}

export function useSaveChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id?: number; input: ChannelInput }) =>
      id ? api.put<Channel>(`/api/notifications/${id}`, input) : api.post<Channel>("/api/notifications", input),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.channels }),
  });
}

export function useDeleteChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete(`/api/notifications/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.channels }),
  });
}

export function useTestChannel() {
  return useMutation({
    mutationFn: (id: number) => api.post<{ ok: boolean; error?: string }>(`/api/notifications/${id}/test`),
  });
}

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: () => api.get<SettingsResponse>("/api/settings") });
}

export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: SettingsResponse["settings"]) => api.put<SettingsResponse>("/api/settings", body),
    onSuccess: (data) => {
      qc.setQueryData(keys.settings, data);
      qc.invalidateQueries({ queryKey: keys.dashboard });
    },
  });
}
