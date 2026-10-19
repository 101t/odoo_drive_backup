import { useEffect, useState, useSyncExternalStore } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { keys } from "../api/hooks";
import type { Progress, Run, RunDetail, RunLog } from "../api/types";

/** Live progress of running jobs, fed by the server-sent event stream. */
const progressByRun = new Map<number, Progress>();
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useLiveProgress(runId: number, fallback: Progress | null = null): Progress | null {
  const live = useSyncExternalStore(subscribe, () => progressByRun.get(runId) ?? null);
  return live ?? fallback;
}

type ServerEvent =
  | { type: "hello" }
  | { type: "resync" }
  | { type: "run_updated"; run: Omit<Run, "progress" | "artifacts"> }
  | { type: "run_progress"; progress: Progress }
  | { type: "run_log"; log: RunLog };

const FINAL = new Set(["success", "partial", "failed", "cancelled"]);

/** Keeps React Query caches in sync with server events. Mount once, when signed in. */
export function useLiveEvents(enabled: boolean) {
  const qc = useQueryClient();

  useEffect(() => {
    if (!enabled) return;
    let source: EventSource | null = null;
    let retryTimer: number | undefined;
    let refreshTimer: number | undefined;
    let stopped = false;

    const refreshLists = () => {
      if (refreshTimer !== undefined) return;
      refreshTimer = window.setTimeout(() => {
        refreshTimer = undefined;
        qc.invalidateQueries({ queryKey: keys.runs });
        qc.invalidateQueries({ queryKey: keys.dashboard });
        qc.invalidateQueries({ queryKey: keys.instances });
        qc.invalidateQueries({ queryKey: keys.destinations });
        qc.invalidateQueries({ queryKey: keys.jobs });
      }, 700);
    };

    const handle = (event: ServerEvent) => {
      switch (event.type) {
        case "hello":
        case "resync":
          refreshLists();
          qc.invalidateQueries({ queryKey: ["run"] });
          break;
        case "run_updated": {
          const run = event.run;
          if (FINAL.has(run.status) && progressByRun.delete(run.id)) notify();
          qc.setQueryData<RunDetail>(keys.run(run.id), (old) => (old ? { ...old, run: { ...old.run, ...run } } : old));
          qc.invalidateQueries({ queryKey: keys.run(run.id) });
          refreshLists();
          break;
        }
        case "run_progress":
          progressByRun.set(event.progress.run_id, event.progress);
          notify();
          break;
        case "run_log":
          qc.setQueryData<RunDetail>(keys.run(event.log.run_id), (old) => {
            if (!old || old.logs.some((l) => l.id === event.log.id)) return old;
            return { ...old, logs: [...old.logs, event.log] };
          });
          break;
      }
    };

    const connect = () => {
      source = new EventSource("/api/events");
      source.onmessage = (message) => {
        try {
          handle(JSON.parse(message.data) as ServerEvent);
        } catch {
          // ignore malformed events
        }
      };
      source.onerror = () => {
        source?.close();
        if (stopped) return;
        // The session may have expired: let the auth query decide.
        qc.invalidateQueries({ queryKey: keys.auth });
        retryTimer = window.setTimeout(connect, 4000);
      };
    };
    connect();

    return () => {
      stopped = true;
      source?.close();
      window.clearTimeout(retryTimer);
      window.clearTimeout(refreshTimer);
    };
  }, [enabled, qc]);
}

/** Re-renders periodically so relative times and durations stay current. */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
