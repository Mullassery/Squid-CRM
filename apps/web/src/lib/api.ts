import type {
  AuthResponse,
  FindManyResponse,
  JsonObject,
  Session,
} from "./types";

const API_BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? "/api";

export class ApiError extends Error {
  status: number;
  code?: string;
  details?: unknown;

  constructor(message: string, status: number, code?: string, details?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

type RequestOptions = Omit<RequestInit, "body"> & {
  body?: JsonObject | JsonObject[] | string;
  query?: Record<string, string | number | boolean | undefined | null>;
};

function buildUrl(path: string, query?: RequestOptions["query"]): string {
  const normalized = path.startsWith("/") ? path : `/${path}`;
  const url = `${API_BASE}${normalized}`;
  if (!query) return url;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === "") continue;
    params.set(key, String(value));
  }
  const serialized = params.toString();
  return serialized ? `${url}?${serialized}` : url;
}

function extractErrorMessage(payload: unknown): {
  message: string;
  code?: string;
} {
  if (!payload || typeof payload !== "object") {
    return { message: "The server returned an unexpected response." };
  }
  const record = payload as Record<string, unknown>;
  const messages = record.messages;
  if (Array.isArray(messages) && messages.length > 0) {
    return {
      message: messages.map(String).join(" "),
      code: typeof record.code === "string" ? record.code : undefined,
    };
  }
  if (typeof record.message === "string") {
    return {
      message: record.message,
      code: typeof record.code === "string" ? record.code : undefined,
    };
  }
  if (typeof record.error === "string") {
    return {
      message: record.error,
      code: typeof record.code === "string" ? record.code : undefined,
    };
  }
  return { message: "The request failed." };
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const { query, body, headers, ...rest } = options;
  const hasBody = body !== undefined;
  const response = await fetch(buildUrl(path, query), {
    ...rest,
    credentials: "include",
    headers: {
      Accept: "application/json",
      ...(hasBody && typeof body !== "string"
        ? { "Content-Type": "application/json" }
        : {}),
      ...(headers ?? {}),
    },
    body:
      body === undefined
        ? undefined
        : typeof body === "string"
          ? body
          : JSON.stringify(body),
  });

  const contentType = response.headers.get("content-type") ?? "";
  const payload: unknown = contentType.includes("application/json")
    ? await response.json().catch(() => null)
    : await response.text().catch(() => "");

  if (!response.ok) {
    const { message, code } = extractErrorMessage(payload);
    throw new ApiError(
      message || `Request failed (${response.status})`,
      response.status,
      code,
      payload,
    );
  }

  return payload as T;
}

export const api = {
  get<T>(path: string, query?: RequestOptions["query"]) {
    return request<T>(path, { method: "GET", query });
  },
  post<T>(path: string, body?: RequestOptions["body"], query?: RequestOptions["query"]) {
    return request<T>(path, { method: "POST", body, query });
  },
  patch<T>(path: string, body?: RequestOptions["body"], query?: RequestOptions["query"]) {
    return request<T>(path, { method: "PATCH", body, query });
  },
  put<T>(path: string, body?: RequestOptions["body"], query?: RequestOptions["query"]) {
    return request<T>(path, { method: "PUT", body, query });
  },
  delete<T>(path: string, query?: RequestOptions["query"]) {
    return request<T>(path, { method: "DELETE", query });
  },
  raw(path: string, options: RequestOptions = {}) {
    return request<unknown>(path, options);
  },
};

export const authApi = {
  signin(email: string, password: string) {
    return api.post<AuthResponse>("/auth/signin", { email, password });
  },
  signup(email: string, password: string, workspaceName: string) {
    return api.post<AuthResponse>("/auth/signup", {
      email,
      password,
      workspaceName,
    });
  },
  signout() {
    return api.post<{ success: boolean }>("/auth/signout");
  },
  me() {
    return api.get<Session>("/rest/me");
  },
};

export function listKey<T>(response: FindManyResponse<T>, key: string): T[] {
  const items = response.data[key];
  return Array.isArray(items) ? items : [];
}

export function formatApiError(error: unknown): string {
  if (error instanceof ApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Something went wrong.";
}

export async function downloadCsv(path: string): Promise<Blob> {
  const response = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    headers: { Accept: "text/csv, application/json" },
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null);
    const { message, code } = extractErrorMessage(payload);
    throw new ApiError(message, response.status, code, payload);
  }
  return response.blob();
}

export function importCsv(path: string, csv: string) {
  return request<{ imported?: number }>(path, {
    method: "POST",
    body: csv,
    headers: { "Content-Type": "text/csv" },
  });
}

export function apiUrl(path: string): string {
  return `${API_BASE}${path.startsWith("/") ? path : `/${path}`}`;
}

export async function uploadToSignedUrl(uploadUrl: string, file: File): Promise<void> {
  const response = await fetch(apiUrl(uploadUrl), {
    method: "PUT",
    credentials: "include",
    headers: { "Content-Type": file.type || "application/octet-stream" },
    body: file,
  });
  if (!response.ok) {
    throw new ApiError(`Upload failed (${response.status})`, response.status);
  }
}
