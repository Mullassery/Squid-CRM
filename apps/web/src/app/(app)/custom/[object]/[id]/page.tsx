"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import { useCustomObject } from "@/hooks/useCustomObject";
import { CustomRecordForm } from "@/components/custom-record-form";
import type { JsonObject } from "@/lib/types";
import { formatDateTime } from "@/lib/format";
import {
  Alert,
  Button,
  ConfirmDialog,
  CopyButton,
  EmptyState,
  LoadingBlock,
  PageHeader,
} from "@/components/ui";
import { Icon } from "@/components/icons";

function display(value: unknown): string {
  if (value === undefined || value === null || value === "") return "—";
  if (typeof value === "object") return JSON.stringify(value, null, 2);
  return String(value);
}

export default function CustomRecordPage() {
  const params = useParams<{ object: string; id: string }>();
  const router = useRouter();
  const objectState = useCustomObject(params.object);
  const recordState = useAsyncData(
    () =>
      gql<{ customObjectRecord: JsonObject | null }>(queries.customRecord, {
        name: params.object,
        id: params.id,
      }),
    [params.object, params.id],
  );
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleteMode, setDeleteMode] = useState<"archive" | "destroy" | null>(null);

  if (objectState.loading || recordState.loading) {
    return <LoadingBlock label="Loading record…" />;
  }

  const object = objectState.data;
  const record = recordState.data?.customObjectRecord;

  if (!object || !record) {
    return (
      <EmptyState
        icon="database"
        title="Record not found"
        action={
          <Link className="btn btn-primary" href={`/custom/${params.object}`}>
            Back to records
          </Link>
        }
      />
    );
  }

  async function update(values: JsonObject) {
    setBusy(true);
    setError(null);
    try {
      await gql(mutations.updateCustomRecord, {
        name: object?.nameSingular ?? params.object,
        id: params.id,
        data: values,
      });
      setNotice("Record updated.");
      setEditing(false);
      recordState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update record.");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleteMode) return;
    setBusy(true);
    setError(null);
    try {
      await gql(mutations.deleteCustomRecord, {
        name: object?.nameSingular ?? params.object,
        id: params.id,
        softDelete: deleteMode === "archive",
      });
      if (deleteMode === "archive") {
        setNotice("Record archived.");
        setDeleteMode(null);
        recordState.reload();
      } else {
        router.push(`/custom/${object?.nameSingular ?? params.object}`);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete record.");
    } finally {
      setBusy(false);
    }
  }

  const fields = object.fieldsList.filter((field) => !field.isSystem);

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href={`/custom/${object.nameSingular}`}>
          <Icon name="chevron-left" size={15} />
          {object.labelPlural}
        </Link>
      </div>
      <PageHeader
        title={
          typeof record.name === "string"
            ? record.name
            : typeof record.title === "string"
              ? record.title
              : String(record.id)
        }
        description={`${object.labelSingular} · ${params.id}`}
        actions={
          editing ? null : (
            <>
              <Button variant="secondary" size="sm" onClick={() => setDeleteMode("archive")}>
                Archive
              </Button>
              <Button variant="primary" size="sm" onClick={() => setEditing(true)}>
                <Icon name="edit" size={15} />
                Edit
              </Button>
              <Button variant="danger-soft" size="sm" onClick={() => setDeleteMode("destroy")}>
                <Icon name="trash" size={15} />
                Delete
              </Button>
            </>
          )
        }
      />

      {notice ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="success" onDismiss={() => setNotice(null)}>
            {notice}
          </Alert>
        </div>
      ) : null}
      {error ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="error" onDismiss={() => setError(null)}>
            {error}
          </Alert>
        </div>
      ) : null}

      {editing ? (
        <div className="card">
          <div className="card-header">
            <h2>Edit record</h2>
          </div>
          <div className="card-body">
            <CustomRecordForm
              fields={object.fieldsList}
              initial={record}
              submitLabel="Save changes"
              busy={busy}
              error={error}
              onSubmit={update}
              onCancel={() => setEditing(false)}
            />
          </div>
        </div>
      ) : (
        <div className="detail-grid">
          <div className="card">
            <div className="card-header">
              <h2>Fields</h2>
            </div>
            <div className="card-body">
              <div className="detail-list">
                {fields.map((field) => (
                  <div className="detail-item" key={field.id}>
                    <div className="detail-label">{field.label}</div>
                    <div className="detail-value" style={{ whiteSpace: "pre-wrap" }}>
                      {display(record[field.name])}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <div className="card">
              <div className="card-header">
                <h2>Record</h2>
              </div>
              <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <div>
                  <div className="detail-label">ID</div>
                  <div className="copy-row">
                    <span className="mono">{params.id}</span>
                    <CopyButton value={params.id} />
                  </div>
                </div>
                <div>
                  <div className="detail-label">Created</div>
                  <div>{formatDateTime(String(record.createdAt ?? record.created_at ?? ""))}</div>
                </div>
                <div>
                  <div className="detail-label">Updated</div>
                  <div>{formatDateTime(String(record.updatedAt ?? record.updated_at ?? ""))}</div>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="card-header">
                <h2>Raw JSON</h2>
              </div>
              <div className="card-body">
                <pre className="json-view">{JSON.stringify(record, null, 2)}</pre>
              </div>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={deleteMode === "archive"}
        title="Archive record?"
        description="The record is soft-deleted and can be restored through the API."
        confirmLabel="Archive"
        danger={false}
        busy={busy}
        onClose={() => setDeleteMode(null)}
        onConfirm={remove}
      />
      <ConfirmDialog
        open={deleteMode === "destroy"}
        title="Delete permanently?"
        description="The record is removed from the workspace table."
        busy={busy}
        onClose={() => setDeleteMode(null)}
        onConfirm={remove}
      />
    </>
  );
}
