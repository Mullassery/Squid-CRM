"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { api, listKey } from "@/lib/api";
import { useAsyncData } from "@/hooks/useAsyncData";
import { useLookups } from "@/hooks/useLookups";
import {
  resourcesByKey,
  type GenericRecord,
  type RelationKind,
  type ResourceConfig,
} from "@/lib/resources";
import { recordValue } from "@/lib/record-values";
import type { FindManyResponse } from "@/lib/types";
import {
  formatCurrency,
  formatDate,
  formatDateTime,
  formatJson,
  shortId,
} from "@/lib/format";
import {
  Alert,
  Button,
  ConfirmDialog,
  CopyButton,
  EmptyState,
  LoadingBlock,
  PageHeader,
  SectionTitle,
  StatusBadge,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const relationResource: Record<RelationKind, string> = {
  person: "people",
  company: "companies",
  opportunity: "opportunities",
  task: "tasks",
  note: "notes",
  member: "",
};

function RelatedRecords({
  title,
  path,
  listKey: key,
  filter,
  target,
}: {
  title: string;
  path: string;
  listKey: string;
  filter: string;
  target: ResourceConfig;
}) {
  const { data, loading } = useAsyncData(
    () =>
      api.get<FindManyResponse<GenericRecord>>(path, {
        limit: 10,
        filter,
        order_by: "createdAt[DescNullsLast]",
      }),
    [path, filter],
  );
  const records = data ? listKey(data, key) : [];
  if (loading) return <LoadingBlock label={`Loading ${title.toLowerCase()}…`} />;
  if (records.length === 0) {
    return <div className="notice">No {title.toLowerCase()} linked yet.</div>;
  }
  return (
    <div className="table-wrap">
      <table className="table">
        <tbody>
          {records.map((record) => (
            <tr key={String(record.id)}>
              <td>
                <Link className="cell-title" href={`/objects/${target.key}/${String(record.id)}`}>
                  {target.title(record)}
                </Link>
                {target.subtitle?.(record) ? (
                  <div className="cell-sub">{target.subtitle(record)}</div>
                ) : null}
              </td>
              <td style={{ textAlign: "right" }}>
                <Link className="btn btn-ghost btn-sm" href={`/objects/${target.key}/${String(record.id)}`}>
                  Open
                  <Icon name="chevron-right" size={14} />
                </Link>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function detailValue(
  record: GenericRecord,
  resource: ResourceConfig,
  key: string,
  kind: string,
  relation: RelationKind | undefined,
  label: (kind: RelationKind, id: string | null | undefined) => string,
) {
  const value = recordValue(record, key);
  if (kind === "relation") {
    const id = typeof value === "string" ? value : null;
    if (!id) return <span className="faint">—</span>;
    const target = relation ? relationResource[relation] : "";
    const text = label(relation ?? "person", id);
    return target ? (
      <Link className="relation-pill" href={`/objects/${target}/${id}`}>
        {text}
      </Link>
    ) : (
      <span className="relation-pill">{text}</span>
    );
  }
  if (value === null || value === undefined || value === "") {
    return <span className="faint">—</span>;
  }
  if (key === "bodyMarkdown") {
    return <div style={{ whiteSpace: "pre-wrap" }}>{String(value)}</div>;
  }
  if (kind === "money") return formatCurrency(value as never);
  if (kind === "date") return formatDate(String(value));
  if (kind === "datetime") return formatDateTime(String(value));
  if (kind === "select") return <StatusBadge value={String(value)} />;
  if (kind === "json") return <pre className="json-view">{formatJson(value as never)}</pre>;
  if (key === "primaryEmail" || key === "domain" || key === "phone") {
    return String(value);
  }
  if (typeof value === "object") return <pre className="json-view">{formatJson(value as never)}</pre>;
  return String(value);
}

export default function ObjectDetailPage() {
  const params = useParams<{ object: string; id: string }>();
  const router = useRouter();
  const resource = resourcesByKey[params.object];
  const { label } = useLookups();
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState<"archive" | "delete" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const { data, error: loadError, loading, reload } = useAsyncData(
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

  if (loading) {
    return <LoadingBlock label={`Loading ${resource.label.toLowerCase()}…`} />;
  }

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

  const deleted = Boolean(data.deletedAt);

  async function archive() {
    setBusy(true);
    setError(null);
    try {
      await api.delete(`${resource.restPath}/${params.id}`, { soft_delete: true });
      setAction(null);
      setNotice("Record archived. You can restore it below.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Archive failed.");
    } finally {
      setBusy(false);
    }
  }

  async function destroy() {
    setBusy(true);
    setError(null);
    try {
      await api.delete(`${resource.restPath}/${params.id}`);
      setAction(null);
      router.push(`/objects/${resource.key}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Delete failed.");
    } finally {
      setBusy(false);
    }
  }

  async function restore() {
    setBusy(true);
    setError(null);
    try {
      await api.patch(`${resource.restPath}/${params.id}/restore`);
      setNotice("Record restored.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Restore failed.");
    } finally {
      setBusy(false);
    }
  }

  const related: { title: string; path: string; listKey: string; filter: string; targetKey: string }[] = [];
  if (resource.key === "people") {
    related.push(
      {
        title: "Opportunities",
        path: "/rest/opportunities",
        listKey: "opportunities",
        filter: `pointOfContactId[eq]:${params.id}`,
        targetKey: "opportunities",
      },
      {
        title: "Task targets",
        path: "/rest/taskTargets",
        listKey: "taskTargets",
        filter: `targetPersonId[eq]:${params.id}`,
        targetKey: "taskTargets",
      },
      {
        title: "Note targets",
        path: "/rest/noteTargets",
        listKey: "noteTargets",
        filter: `targetPersonId[eq]:${params.id}`,
        targetKey: "noteTargets",
      },
    );
  }
  if (resource.key === "companies") {
    related.push(
      {
        title: "People",
        path: "/rest/people",
        listKey: "people",
        filter: `companyId[eq]:${params.id}`,
        targetKey: "people",
      },
      {
        title: "Opportunities",
        path: "/rest/opportunities",
        listKey: "opportunities",
        filter: `companyId[eq]:${params.id}`,
        targetKey: "opportunities",
      },
    );
  }
  if (resource.key === "opportunities") {
    related.push(
      {
        title: "Task targets",
        path: "/rest/taskTargets",
        listKey: "taskTargets",
        filter: `targetOpportunityId[eq]:${params.id}`,
        targetKey: "taskTargets",
      },
      {
        title: "Note targets",
        path: "/rest/noteTargets",
        listKey: "noteTargets",
        filter: `targetOpportunityId[eq]:${params.id}`,
        targetKey: "noteTargets",
      },
    );
  }
  if (resource.key === "tasks") {
    related.push({
      title: "Task targets",
      path: "/rest/taskTargets",
      listKey: "taskTargets",
      filter: `taskId[eq]:${params.id}`,
      targetKey: "taskTargets",
    });
  }
  if (resource.key === "notes") {
    related.push({
      title: "Note targets",
      path: "/rest/noteTargets",
      listKey: "noteTargets",
      filter: `noteId[eq]:${params.id}`,
      targetKey: "noteTargets",
    });
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
        title={resource.title(data)}
        description={resource.subtitle?.(data) || undefined}
        actions={
          <>
            {deleted ? (
              <Button size="sm" variant="secondary" onClick={restore} disabled={busy}>
                <Icon name="refresh" size={15} />
                Restore
              </Button>
            ) : (
              <>
                <Button size="sm" variant="secondary" onClick={() => setAction("archive")} disabled={busy}>
                  Archive
                </Button>
                <Link className="btn btn-primary btn-sm" href={`/objects/${resource.key}/${params.id}/edit`}>
                  <Icon name="edit" size={15} />
                  Edit
                </Link>
              </>
            )}
            <Button size="sm" variant="danger-soft" onClick={() => setAction("delete")} disabled={busy}>
              <Icon name="trash" size={15} />
              Delete
            </Button>
          </>
        }
      />

      {deleted ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="warning">
            This record is archived{data.deletedAt ? ` (${formatDateTime(String(data.deletedAt))})` : ""}.
            Restore it to make it editable again.
          </Alert>
        </div>
      ) : null}
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

      <div className="detail-grid">
        <div className="card">
          <div className="card-header">
            <h2>Details</h2>
            <span className="badge accent">
              <Icon name="database" size={12} />
              {resource.objectName}
            </span>
          </div>
          <div className="card-body">
            <div className="detail-list">
              {resource.fields.map((field) => (
                <div className="detail-item" key={field.key}>
                  <div className="detail-label">{field.label}</div>
                  <div className="detail-value">
                    {detailValue(data, resource, field.key, field.kind, field.relation, label)}
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
            <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div>
                <div className="detail-label">Record ID</div>
                <div className="copy-row">
                  <span className="mono">{shortId(String(data.id))}</span>
                  <CopyButton value={String(data.id)} />
                </div>
              </div>
              <div className="detail-list" style={{ gridTemplateColumns: "1fr" }}>
                <div className="detail-item">
                  <div className="detail-label">Created</div>
                  <div className="detail-value">{formatDateTime(String(data.createdAt))}</div>
                </div>
                <div className="detail-item">
                  <div className="detail-label">Updated</div>
                  <div className="detail-value">{formatDateTime(String(data.updatedAt))}</div>
                </div>
                {data.searchVector ? (
                  <div className="detail-item">
                    <div className="detail-label">Search vector</div>
                    <div className="detail-value small muted">{String(data.searchVector)}</div>
                  </div>
                ) : null}
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-header">
              <h2>API</h2>
            </div>
            <div className="card-body">
              <p className="muted small" style={{ marginBottom: 8 }}>
                Record URL
              </p>
              <div className="code-block">{`${resource.restPath}/${String(data.id)}`}</div>
            </div>
          </div>
        </div>
      </div>

      {related.length > 0 ? (
        <>
          <SectionTitle title="Related records" />
          <div className="grid cols-2">
            {related.map((item) => {
              const target = resourcesByKey[item.targetKey];
              if (!target) return null;
              return (
                <div className="card" key={item.title}>
                  <div className="card-header">
                    <h3>{item.title}</h3>
                  </div>
                  <RelatedRecords
                    title={item.title}
                    path={item.path}
                    listKey={item.listKey}
                    filter={item.filter}
                    target={target}
                  />
                </div>
              );
            })}
          </div>
        </>
      ) : null}

      <ConfirmDialog
        open={action === "archive"}
        title="Archive record?"
        description="The record is soft-deleted and can be restored later."
        confirmLabel="Archive"
        danger={false}
        busy={busy}
        onClose={() => setAction(null)}
        onConfirm={archive}
      />
      <ConfirmDialog
        open={action === "delete"}
        title="Delete permanently?"
        description="This permanently removes the record from the workspace."
        confirmLabel="Delete permanently"
        busy={busy}
        onClose={() => setAction(null)}
        onConfirm={destroy}
      />
    </>
  );
}
