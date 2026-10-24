"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { api } from "@/lib/api";
import { useAsyncData } from "@/hooks/useAsyncData";
import { resourcesByKey, type FormValues, type GenericRecord } from "@/lib/resources";
import { RecordForm } from "@/components/record-form";
import { Alert, EmptyState, LoadingBlock, PageHeader } from "@/components/ui";
import { Icon } from "@/components/icons";

export default function EditObjectPage() {
  const params = useParams<{ object: string; id: string }>();
  const router = useRouter();
  const resource = resourcesByKey[params.object];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data, loading, error: loadError } = useAsyncData(
    async () => {
      if (!resource) return null;
      const response = await api.get<{ data: Record<string, GenericRecord> }>(
        `${resource.restPath}/${params.id}`,
      );
      return response.data[resource.objectName] ?? Object.values(response.data)[0] ?? null;
    },
    [resource?.key, params.id],
  );

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

  if (loading) return <LoadingBlock label={`Loading ${resource.label.toLowerCase()}…`} />;

  if (loadError || !data) {
    return (
      <EmptyState
        icon="search"
        title="Record not found"
        description={loadError ?? undefined}
        action={
          <Link className="btn btn-primary" href={`/objects/${resource.key}`}>
            Back to {resource.labelPlural.toLowerCase()}
          </Link>
        }
      />
    );
  }

  async function submit(values: FormValues) {
    setBusy(true);
    setError(null);
    try {
      await api.patch(
        `${resource.restPath}/${params.id}`,
        resource.buildPayload(values),
      );
      router.push(`/objects/${resource.key}/${params.id}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Update failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href={`/objects/${resource.key}/${params.id}`}>
          <Icon name="chevron-left" size={15} />
          {resource.title(data)}
        </Link>
      </div>
      <PageHeader
        title={`Edit ${resource.label.toLowerCase()}`}
        description={resource.title(data)}
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
            record={data}
            submitLabel="Save changes"
            busy={busy}
            error={error}
            onSubmit={submit}
            onCancel={() => router.push(`/objects/${resource.key}/${params.id}`)}
          />
        </div>
      </div>
    </>
  );
}
