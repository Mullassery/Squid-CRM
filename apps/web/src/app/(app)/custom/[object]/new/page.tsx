"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { gql, mutations } from "@/lib/graphql";
import { useCustomObject } from "@/hooks/useCustomObject";
import { CustomRecordForm } from "@/components/custom-record-form";
import type { JsonObject } from "@/lib/types";
import { Alert, EmptyState, LoadingBlock, PageHeader } from "@/components/ui";
import { Icon } from "@/components/icons";

export default function NewCustomRecordPage() {
  const params = useParams<{ object: string }>();
  const router = useRouter();
  const objectState = useCustomObject(params.object);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (objectState.loading) return <LoadingBlock label="Loading object…" />;

  const object = objectState.data;

  if (!object) {
    return (
      <EmptyState
        icon="database"
        title="Custom object not found"
        action={
          <Link className="btn btn-primary" href="/settings/data">
            Custom objects
          </Link>
        }
      />
    );
  }

  async function submit(values: JsonObject) {
    setBusy(true);
    setError(null);
    try {
      const result = await gql<{ createCustomObjectRecord: JsonObject }>(
        mutations.createCustomRecord,
        { name: object?.nameSingular ?? params.object, data: values },
      );
      const id = result.createCustomObjectRecord.id;
      router.push(`/custom/${object?.nameSingular ?? params.object}/${String(id)}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create record.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href={`/custom/${object.nameSingular}`}>
          <Icon name="chevron-left" size={15} />
          {object.labelPlural}
        </Link>
      </div>
      <PageHeader
        title={`New ${object.labelSingular.toLowerCase()}`}
        description={`Fields are defined by the ${object.labelSingular} metadata.`}
      />
      {error ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="error">{error}</Alert>
        </div>
      ) : null}
      <div className="card">
        <div className="card-body">
          <CustomRecordForm
            fields={object.fieldsList}
            submitLabel={`Create ${object.labelSingular.toLowerCase()}`}
            busy={busy}
            error={error}
            onSubmit={submit}
            onCancel={() => router.push(`/custom/${object.nameSingular}`)}
          />
        </div>
      </div>
    </>
  );
}
