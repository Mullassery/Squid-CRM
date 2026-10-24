"use client";

import { useState } from "react";
import { useLookups } from "@/hooks/useLookups";
import {
  emptyForm,
  type FormValues,
  type GenericRecord,
  type ResourceConfig,
} from "@/lib/resources";
import { Alert, Button, Field, Spinner } from "@/components/ui";

export function RecordForm({
  resource,
  record,
  submitLabel,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  resource: ResourceConfig;
  record?: GenericRecord | null;
  submitLabel: string;
  busy?: boolean;
  error?: string | null;
  onSubmit: (values: FormValues) => void | Promise<void>;
  onCancel: () => void;
}) {
  const { options, loading: lookupsLoading } = useLookups();
  const [values, setValues] = useState<FormValues>(() =>
    record ? resource.toForm(record) : emptyForm(resource),
  );
  const [missing, setMissing] = useState<string[]>([]);

  function update(key: string, value: string) {
    setValues((current) => ({ ...current, [key]: value }));
  }

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const required = resource.fields
      .filter((field) => field.required)
      .filter((field) => !values[field.key]?.trim())
      .map((field) => field.key);
    setMissing(required);
    if (required.length > 0) return;
    void onSubmit(values);
  }

  return (
    <form onSubmit={submit}>
      {error ? (
        <div className="form-error">
          <Alert tone="error">{error}</Alert>
        </div>
      ) : null}
      <div className="form-grid">
        {resource.fields.map((field) => {
          const value = values[field.key] ?? "";
          const invalid = missing.includes(field.key);
          return (
            <Field
              key={field.key}
              label={field.label}
              required={field.required}
              help={invalid ? "This field is required." : field.help}
              wide={field.wide}
            >
              {field.kind === "textarea" ? (
                <textarea
                  className="textarea"
                  value={value}
                  placeholder={field.placeholder}
                  onChange={(event) => update(field.key, event.target.value)}
                />
              ) : field.kind === "select" ? (
                <select
                  className="select"
                  value={value}
                  onChange={(event) => update(field.key, event.target.value)}
                >
                  {!field.required && field.options?.[0]?.value !== "" ? (
                    <option value="">None</option>
                  ) : null}
                  {field.options?.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              ) : field.kind === "relation" ? (
                <select
                  className="select"
                  value={value}
                  disabled={lookupsLoading}
                  onChange={(event) => update(field.key, event.target.value)}
                >
                  <option value="">None</option>
                  {options[field.relation ?? "person"]?.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  className="input"
                  type={
                    field.kind === "email"
                      ? "email"
                      : field.kind === "number" || field.kind === "money"
                        ? "number"
                        : field.kind === "date"
                          ? "date"
                          : field.kind === "datetime"
                            ? "datetime-local"
                            : "text"
                  }
                  step={field.kind === "money" ? "0.01" : undefined}
                  value={value}
                  placeholder={field.placeholder}
                  onChange={(event) => update(field.key, event.target.value)}
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
