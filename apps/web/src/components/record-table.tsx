"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { useLookups } from "@/hooks/useLookups";
import type { GenericRecord, RelationKind, ResourceConfig } from "@/lib/resources";
import type { PageInfo } from "@/lib/types";
import { recordValue } from "@/lib/record-values";
import {
  formatCurrency,
  formatDate,
  formatDateTime,
  shortId,
} from "@/lib/format";
import {
  Alert,
  Button,
  ConfirmDialog,
  EmptyState,
  IconButton,
  LoadingBlock,
  StatusBadge,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const relationResource: Record<string, string> = {
  person: "people",
  company: "companies",
  opportunity: "opportunities",
  task: "tasks",
  note: "notes",
};

function titleKeys(resourceKey: string): string[] {
  switch (resourceKey) {
    case "people":
      return ["firstName", "lastName"];
    case "companies":
    case "opportunities":
      return ["name"];
    case "tasks":
    case "notes":
      return ["title"];
    default:
      return [];
  }
}

export function RecordTable({
  resource,
  records,
  loading,
  error,
  totalCount,
  pageInfo,
  offset,
  pageSize,
  onPrevious,
  onNext,
  onArchive,
}: {
  resource: ResourceConfig;
  records: GenericRecord[];
  loading: boolean;
  error?: string | null;
  totalCount: number;
  pageInfo?: PageInfo;
  offset: number;
  pageSize: number;
  onPrevious: () => void;
  onNext: () => void;
  onArchive: (record: GenericRecord) => Promise<void>;
}) {
  const { label } = useLookups();
  const [search, setSearch] = useState("");
  const [archiveTarget, setArchiveTarget] = useState<GenericRecord | null>(null);
  const [archiving, setArchiving] = useState(false);

  const skip = useMemo(() => new Set(titleKeys(resource.key)), [resource.key]);
  const columns = resource.fields.filter(
    (field) => field.list && !skip.has(field.key) && field.kind !== "json",
  );

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return records;
    return records.filter((record) => {
      const haystack = [
        resource.title(record),
        resource.subtitle?.(record),
        ...columns.map((column) => {
          const value = recordValue(record, column.key);
          if (value === null || value === undefined) return "";
          if (column.kind === "relation" && typeof value === "string") {
            return label(column.relation ?? "person", value);
          }
          return String(value);
        }),
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });
  }, [records, search, resource, columns, label]);

  function renderCell(
    record: GenericRecord,
    key: string,
    kind: string,
    relation?: RelationKind,
  ) {
    const value = recordValue(record, key);
    if (kind === "relation") {
      const id = typeof value === "string" ? value : null;
      if (!id) return <span className="faint">—</span>;
      const path = relation ? relationResource[relation] : undefined;
      const display = label(relation ?? "person", id);
      return path ? (
        <Link className="relation-pill" href={`/objects/${path}/${id}`}>
          {display}
        </Link>
      ) : (
        <span className="relation-pill">{display}</span>
      );
    }
    if (value === null || value === undefined || value === "") {
      return <span className="faint">—</span>;
    }
    if (kind === "money") return formatCurrency(value as never);
    if (kind === "date") return formatDate(String(value));
    if (kind === "datetime") return formatDateTime(String(value));
    if (kind === "select") return <StatusBadge value={String(value)} />;
    if (key === "primaryEmail") return String(value);
    if (key === "domain") return String(value);
    if (typeof value === "object") return shortId(JSON.stringify(value));
    return String(value);
  }

  return (
    <div className="card">
      <div className="list-toolbar">
        <div style={{ position: "relative", flex: 1, minWidth: 220 }}>
          <input
            className="input"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={`Filter ${resource.labelPlural.toLowerCase()} on this page…`}
          />
        </div>
        <div className="spacer" />
        <span className="faint small">
          {filtered.length === records.length
            ? `${totalCount.toLocaleString()} records`
            : `${filtered.length} of ${records.length} on this page`}
        </span>
        <Link className="btn btn-primary btn-sm" href={`/objects/${resource.key}/new`}>
          <Icon name="plus" size={15} />
          New {resource.label.toLowerCase()}
        </Link>
      </div>

      {error ? (
        <div style={{ padding: 14 }}>
          <Alert tone="error">{error}</Alert>
        </div>
      ) : null}

      {loading ? (
        <LoadingBlock label={`Loading ${resource.labelPlural.toLowerCase()}…`} />
      ) : records.length === 0 ? (
        <EmptyState
          icon="database"
          title={`No ${resource.labelPlural.toLowerCase()} yet`}
          description={`Create your first ${resource.label.toLowerCase()} to get started.`}
          action={
            <Link className="btn btn-primary" href={`/objects/${resource.key}/new`}>
              <Icon name="plus" size={15} />
              New {resource.label.toLowerCase()}
            </Link>
          }
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon="search"
          title="No matches on this page"
          description="Try a different filter or load the next page."
        />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>{resource.label}</th>
                {columns.map((column) => (
                  <th key={column.key}>{column.label}</th>
                ))}
                <th style={{ textAlign: "right" }}>Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((record) => (
                <tr key={String(record.id)}>
                  <td>
                    <Link href={`/objects/${resource.key}/${record.id}`} className="cell-title">
                      {resource.title(record)}
                    </Link>
                    {resource.subtitle?.(record) ? (
                      <div className="cell-sub">{resource.subtitle(record)}</div>
                    ) : null}
                  </td>
                  {columns.map((column) => (
                    <td key={column.key}>{renderCell(record, column.key, column.kind, column.relation)}</td>
                  ))}
                  <td>
                    <div className="row-actions">
                      <Link
                        className="icon-btn"
                        href={`/objects/${resource.key}/${record.id}/edit`}
                        aria-label="Edit"
                        title="Edit"
                      >
                        <Icon name="edit" size={15} />
                      </Link>
                      <IconButton
                        icon="trash"
                        label="Archive"
                        onClick={() => setArchiveTarget(record)}
                      />
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
          {totalCount === 0
            ? "No records"
            : `${offset + 1}–${Math.min(offset + pageSize, offset + records.length)} of ${totalCount.toLocaleString()}`}
        </span>
        <div className="inline-actions">
          <Button
            size="sm"
            variant="secondary"
            onClick={onPrevious}
            disabled={offset === 0 || loading}
          >
            <Icon name="chevron-left" size={14} />
            Previous
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={onNext}
            disabled={!pageInfo?.hasNextPage || loading}
          >
            Next
            <Icon name="chevron-right" size={14} />
          </Button>
        </div>
      </div>

      <ConfirmDialog
        open={Boolean(archiveTarget)}
        title={`Archive ${resource.label.toLowerCase()}?`}
        description="The record will be soft-deleted and can be restored from the detail page."
        confirmLabel="Archive"
        danger={false}
        busy={archiving}
        onClose={() => setArchiveTarget(null)}
        onConfirm={() => {
          if (!archiveTarget) return;
          setArchiving(true);
          void onArchive(archiveTarget)
            .then(() => setArchiveTarget(null))
            .finally(() => setArchiving(false));
        }}
      />    </div>
  );
}
