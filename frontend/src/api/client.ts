import { getLocale, t, ts } from "../i18n";

export class ApiError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

export const UNAUTHORIZED_EVENT = "odb:unauthorized";

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      credentials: "same-origin",
      headers: {
        // Lets the server describe schedules in the UI language.
        "Accept-Language": getLocale(),
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(t("error.unreachable"), 0);
  }

  const text = await response.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { error: text };
    }
  }
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith("/api/auth/")) {
      window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    }
    const message =
      (data as { error?: string } | null)?.error ?? `${response.status} ${response.statusText}`;
    throw new ApiError(message, response.status);
  }
  return data as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  put: <T>(path: string, body: unknown) => request<T>("PUT", path, body),
  delete: <T>(path: string) => request<T>("DELETE", path),
};

/** The message of an error, translated when it comes from the server. */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return ts(error.message);
  return ts(String(error));
}
