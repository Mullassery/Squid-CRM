import type { Currency, JsonValue, Links, Phones } from "./types";

export function formatDate(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  }).format(date);
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function toDateInput(value: string | null | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toISOString().slice(0, 10);
}

export function toDateTimeInput(value: string | null | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toISOString().slice(0, 16);
}

export function fromDateInput(value: string): string | undefined {
  if (!value) return undefined;
  const date = new Date(`${value}T00:00:00`);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

export function fromDateTimeInput(value: string): string | undefined {
  if (!value) return undefined;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

export function formatCurrency(value: Currency | null | undefined): string {
  if (!value || value.amountMicros === null || value.amountMicros === undefined) {
    return "—";
  }
  const amount = value.amountMicros / 1_000_000;
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: value.currencyCode || "USD",
      maximumFractionDigits: 2,
    }).format(amount);
  } catch {
    return `${amount.toFixed(2)} ${value.currencyCode}`;
  }
}

export function currencyInputValue(value: Currency | null | undefined): string {
  if (!value || value.amountMicros === null || value.amountMicros === undefined) {
    return "";
  }
  return String(value.amountMicros / 1_000_000);
}

export function formatFullName(
  value: { firstName?: string | null; lastName?: string | null } | null | undefined,
): string {
  if (!value) return "—";
  const parts = [value.firstName, value.lastName].filter(
    (part): part is string => Boolean(part && part.trim()),
  );
  return parts.length ? parts.join(" ") : "Untitled";
}

export function formatEmail(value: { primaryEmail?: string | null } | null | undefined): string {
  return value?.primaryEmail?.trim() || "—";
}

export function formatLinks(value: Links | null | undefined): string {
  return value?.primaryLinkUrl?.trim() || "—";
}

export function formatPhones(value: Phones | null | undefined): string {
  if (!value) return "—";
  return (
    [value.primaryPhoneCountryCode, value.primaryPhoneNumber]
      .filter(Boolean)
      .join(" ")
      .trim() || "—"
  );
}

export function formatJson(value: JsonValue | undefined): string {
  if (value === undefined || value === null) return "—";
  if (typeof value === "string") return value || "—";
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value, null, 2);
}

export function splitList(value: string | undefined | null): string[] {
  if (!value) return [];
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function humanize(value: string): string {
  return value
    .replace(/_/g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

export function shortId(value: string | null | undefined): string {
  if (!value) return "—";
  return value.length > 10 ? `${value.slice(0, 8)}…` : value;
}
