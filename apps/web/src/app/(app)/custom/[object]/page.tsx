"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import { useCustomObject } from "@/hooks/useCustomObject";
import type { JsonObject, JsonValue } from "@/lib/types";
import { formatDate } from "@/lib/format";
import {
  Alert,
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  LoadingBlock,
  PageHeader,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const PAGE_SIZE = 25;

function display(value: JsonValue | undefined): string {
  if (value === undefined || value === null || value === "") return "—";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function recordTitle(record: JsonObject): string {
  for (const key of ["name", "title", "label", "subject"]) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  const firstString = Object.values(record).find(
    (value) => typeof value === "string" && value.length > 0,
  );
  return typeof firstString === "string" ? firstString : String(record.id);
}

export default function CustomObjectListPage() {
  const params = useParams<{ object: string }>();
  const objectState = useCustomObject(params.object);
  const [offset, setOffset] = useState(0);
  const [search, setSearch] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<JsonObject | null>(null);
  const [deleting, setDeleting] = useState(false);

  const records = useAsyncData(
    () =>
      gql<{ customObjectRecords: JsonObject[] }>(queries.customRecords, {
        name: params.object,
        limit: PAGE_SIZE + 1,
        offset,
      }),
    [params.object, offset],
  );

  const object = objectState.data;
  const rows = records.data?.customObjectRecords ?? [];
  const hasNext = rows.length > PAGE_SIZE;
  const pageRows = rows.slice(0, PAGE_SIZE);
  const columns = useMemo(
    () => (object?.fieldsList ?? []).filter((field) => !field.isSystem).slice(0, 5),
    [object],
  );

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return pageRows;
    return pageRows.filter((record) =>
      Object.values(record).some((value) =>
        String(value ?? "").toLowerCase().includes(needle),
      ),
    );
  }, [pageRows, search]);

  if (objectState.loading) return <LoadingBlock label="Loading object…" />;

  if (!object) {
    return (
      <EmptyState
        icon="database"
        title="Custom object not found"
        description="Create it in the data model settings first."
        action={
          <Link className="btn btn-primary" href="/settings/metadata">
            Open data model
          </Link>
        }
      />
    );
  }

  async function remove() {
    if (!deleteTarget) return;
    setDeleting(true);
    setError(null);
    try {
      await gql(mutations.deleteCustomRecord, {
        name: object?.nameSingular ?? params.object,
        id: deleteTarget.id,
        softDelete: true,
      });
      setNotice("Record archived.");
      setDeleteTarget(null);
      records.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not archive record.");
    } finally {
      setDeleting(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href="/settings/data">
          <Icon name="chevron-left" size={15} />
          Custom objects
        </Link>
      </div>
      <PageHeader
        title={object.labelPlural}
        description={`${object.description || "Custom object"} · generic JSON records in a workspace table.`}
        actions={
          <>
            <Link className="btn btn-secondary btn-sm" href={`/settings/metadata/${object.id}`}>
              <Icon name="settings" size={15} />
              Schema
            </Link>
            <Link className="btn btn-primary btn-sm" href={`/custom/${object.nameSingular}/new`}>
              <Icon name="plus" size={15} />
              New {object.labelSingular.toLowerCase()}
            </Link>
          </>
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

      <div className="card">
        <div className="list-toolbar">
          <input
            className="input"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={`Filter ${object.labelPlural.toLowerCase()}…`}
          />
          <div className="spacer" />
          <Badge tone="accent">{object.nameSingular}</Badge>
        </div>

        {records.loading ? (
          <LoadingBlock label="Loading records…" />
        ) : pageRows.length === 0 ? (
          <EmptyState
            icon="database"
            title={`No ${object.labelPlural.toLowerCase()} yet`}
            description={`Create your first ${object.labelSingular.toLowerCase()}.`}
            action={
              <Link className="btn btn-primary" href={`/custom/${object.nameSingular}/new`}>
                <Icon name="plus" size={15} />
                New {object.labelSingular.toLowerCase()}
              </Link>
            }
          />
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Record</th>
                  {columns.map((field) => (
                    <th key={field.id}>{field.label}</th>
                  ))}
                  <th>Created</th>
                  <th style={{ textAlign: "right" }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((record) => (
                  <tr key={String(record.id)}>
                    <td>
                      <Link
                        className="cell-title"
                        href={`/custom/${object.nameSingular}/${record.id}`}
                      >
                        {recordTitle(record)}
                      </Link>
                      <div className="cell-sub mono">{String(record.id).slice(0, 8)}…</div>
                    </td>
                    {columns.map((field) => (
                      <td key={field.id}>{display(record[field.name])}</td>
                    ))}
                    <td className="muted">{formatDate(String(record.createdAt ?? record.created_at ?? ""))}</td>
                    <td>
                      <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                        <Link
                          className="icon-btn"
                          href={`/custom/${object.nameSingular}/${record.id}`}
                          aria-label="Open record"
                        >
                          <Icon name="edit" size={15} />
                        </Link>
                        <button
                          className="icon-btn"
                          aria-label="Archive record"
                          onClick={() => setDeleteTarget(record)}
                        >
                          <Icon name="trash" size={15} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <div className="pagination">
          <span className="page-meta">
            Showing {pageRows.length === 0 ? 0 : offset + 1}–{offset + pageRows.length}
          </span>
          <div className="inline-actions">
            <Button
              size="sm"
              variant="secondary"
              disabled={offset === 0 || records.loading}
              onClick={() => setOffset((value) => Math.max(0, value - PAGE_SIZE))}
            >
              <Icon name="chevron-left" size={14} />
              Previous
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={!hasNext || records.loading}
              onClick={() => setOffset((value) => value + PAGE_SIZE)}
            >
              Next
              <Icon name="chevron-right" size={14} />
            </Button>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="Archive record?"
        description="The record is soft-deleted through the custom-object API."
        confirmLabel="Archive"
        danger={false}
        busy={deleting}
        onClose={() => setDeleteTarget(null)}
        onConfirm={remove}
      />
    </>
  );
}
