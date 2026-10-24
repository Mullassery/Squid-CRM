"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { api } from "@/lib/api";
import { resourcesByKey, type FormValues, type GenericRecord } from "@/lib/resources";
import { RecordForm } from "@/components/record-form";
import { Alert, EmptyState, PageHeader } from "@/components/ui";
import { Icon } from "@/components/icons";

export default function NewObjectPage() {
  const params = useParams<{ object: string }>();
  const router = useRouter();
  const resource = resourcesByKey[params.object];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!resource) {
    return (
      <EmptyState
        icon="search"
        title="Object not found"
        action={
          <Link className="btn btn-primary" href="/dashboard">
            Back to dashboard
          </Link>
        }
      />
    );
  }

  async function submit(values: FormValues) {
    setBusy(true);
    setError(null);
    try {
      const response = await api.post<{ data: Record<string, GenericRecord> }>(
        resource.restPath,
        resource.buildPayload(values),
      );
      const created = response.data[resource.createKey];
      if (created?.id) {
        router.push(`/objects/${resource.key}/${String(created.id)}`);
      } else {
        router.push(`/objects/${resource.key}`);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Create failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href={`/objects/${resource.key}`}>
          <Icon name="chevron-left" size={15} />
          {resource.labelPlural}
        </Link>
      </div>
      <PageHeader
        title={`New ${resource.label.toLowerCase()}`}
        description={`Create a ${resource.label.toLowerCase()}. Required fields are marked with an asterisk.`}
      />
      {error ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="error">{error}</Alert>
        </div>
      ) : null}
      <div className="card">
        <div className="card-body">
          <RecordForm
            resource={resource}
            submitLabel={`Create ${resource.label.toLowerCase()}`}
            busy={busy}
            error={error}
            onSubmit={submit}
            onCancel={() => router.push(`/objects/${resource.key}`)}
          />
        </div>
      </div>
    </>
  );
}
