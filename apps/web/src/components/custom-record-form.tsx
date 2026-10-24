"use client";

import { useState } from "react";
import type { FieldMetadata, JsonObject, JsonValue } from "@/lib/types";
import { Alert, Button, Field, Spinner } from "@/components/ui";

export type CustomValues = Record<string, string>;

function initialValue(field: FieldMetadata, value: JsonValue | undefined): string {
  if (value === undefined || value === null) return "";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function convert(field: FieldMetadata, raw: string): JsonValue | undefined {
  const value = raw.trim();
  if (!value) return undefined;
  switch (field.type) {
    case "BOOLEAN":
      return value === "true";
    case "NUMBER":
    case "NUMERIC":
    case "RATING": {
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : undefined;
    }
    case "MULTI_SELECT":
    case "ARRAY":
      return value
        .split(",")
        .map((item) => item.trim())
        .filter(Boolean);
    case "RAW_JSON":
      try {
        return JSON.parse(value) as JsonValue;
      } catch {
        return value;
      }
    case "DATE":
    case "DATE_TIME": {
      const date = new Date(value);
      return Number.isNaN(date.getTime()) ? value : date.toISOString();
    }
    default:
      return value;
  }
}

export function CustomRecordForm({
  fields,
  initial,
  submitLabel,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  fields: FieldMetadata[];
  initial?: JsonObject | null;
  submitLabel: string;
  busy?: boolean;
  error?: string | null;
  onSubmit: (values: JsonObject) => void | Promise<void>;
  onCancel: () => void;
}) {
  const editable = fields.filter(
    (field) => !field.isSystem && field.type !== "MORPH_RELATION",
  );
  const [values, setValues] = useState<CustomValues>(() =>
    Object.fromEntries(
      editable.map((field) => [
        field.name,
        initialValue(field, initial?.[field.name] as JsonValue | undefined),
      ]),
    ),
  );
  const [missing, setMissing] = useState<string[]>([]);

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const required = editable
      .filter((field) => !field.isNullable)
      .filter((field) => !values[field.name]?.trim())
      .map((field) => field.name);
    setMissing(required);
    if (required.length > 0) return;
    const payload: JsonObject = {};
    for (const field of editable) {
      const converted = convert(field, values[field.name] ?? "");
      if (converted !== undefined) payload[field.name] = converted;
    }
    void onSubmit(payload);
  }

  if (editable.length === 0) {
    return <Alert tone="warning">This object has no writable fields yet.</Alert>;
  }

  return (
    <form onSubmit={submit}>
      {error ? (
        <div className="form-error">
          <Alert tone="error">{error}</Alert>
        </div>
      ) : null}
      <div className="form-grid">
        {editable.map((field) => {
          const value = values[field.name] ?? "";
          const invalid = missing.includes(field.name);
          return (
            <Field
              key={field.id}
              label={field.label}
              required={!field.isNullable}
              help={invalid ? "This field is required." : field.description ?? undefined}
              wide={field.type === "RICH_TEXT" || field.type === "RAW_JSON"}
            >
              {field.type === "BOOLEAN" ? (
                <select
                  className="select"
                  value={value}
                  onChange={(event) =>
                    setValues((current) => ({ ...current, [field.name]: event.target.value }))
                  }
                >
                  <option value="">Not set</option>
                  <option value="true">True</option>
                  <option value="false">False</option>
                </select>
              ) : field.type === "RICH_TEXT" || field.type === "RAW_JSON" ? (
                <textarea
                  className="textarea mono"
                  value={value}
                  onChange={(event) =>
                    setValues((current) => ({ ...current, [field.name]: event.target.value }))
                  }
                  style={{ minHeight: 130 }}
                />
              ) : (
                <input
                  className="input"
                  type={
                    field.type === "NUMBER" || field.type === "NUMERIC" || field.type === "RATING"
                      ? "number"
                      : field.type === "DATE"
                        ? "date"
                        : field.type === "DATE_TIME"
                          ? "datetime-local"
                          : "text"
                  }
                  value={value}
                  placeholder={field.type === "RELATION" ? "Record UUID" : undefined}
                  onChange={(event) =>
                    setValues((current) => ({ ...current, [field.name]: event.target.value }))
                  }
                />
              )}
            </Field>
          );
        })}
      </div>
      <div className="form-actions">
        <Button type="button" variant="secondary" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? <Spinner /> : null}
          {submitLabel}
        </Button>
      </div>
    </form>
  );
}
