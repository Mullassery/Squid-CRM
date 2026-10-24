"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ObjectMetadata, ViewRecord } from "@/lib/types";
import {
  Alert,
  Badge,
  Button,
  ConfirmDialog,
  LoadingBlock,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

export default function ViewDetailPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const viewState = useAsyncData(
    () => gql<{ getView: ViewRecord | null }>(queries.view, { id: params.id }),
    [params.id],
  );
  const objects = useAsyncData(
    () =>
      gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(queries.objects, {
        first: 100,
      }),
    [],
  );
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyField, setBusyField] = useState<string | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  if (viewState.loading || objects.loading) return <LoadingBlock label="Loading view…" />;

  const view = viewState.data?.getView;
  if (!view) {
    return (
      <div className="card">
        <div className="empty-state">
          <h3>View not found</h3>
          <Link className="btn btn-primary" href="/settings/views">
            Back to views
          </Link>
        </div>
      </div>
    );
  }

  const viewId = view.id;
  const objectList = objects.data?.objects.edges.map((edge) => edge.node) ?? [];
  const object = objectList.find((item) => item.id === view.objectMetadataId);
  const fieldById = new Map((object?.fieldsList ?? []).map((field) => [field.id, field]));

  async function toggleVisible(fieldId: string, isVisible: boolean) {
    setBusyField(fieldId);
    setError(null);
    try {
      await gql(mutations.updateViewField, {
        input: { id: fieldId, update: { isVisible } },
      });
      viewState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update field.");
    } finally {
      setBusyField(null);
    }
  }

  async function remove() {
    setDeleting(true);
    setError(null);
    try {
      await gql(mutations.deleteView, { id: viewId });
      router.push("/settings/views");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete view.");
      setDeleting(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href="/settings/views">
          <Icon name="chevron-left" size={15} />
          Views
        </Link>
      </div>
      <PageHeader
        title={view.name}
        description={`${object?.labelPlural ?? "Unknown object"} · ${view.type}`}
        actions={
          <Button variant="danger-soft" size="sm" onClick={() => setDeleteOpen(true)}>
            <Icon name="trash" size={15} />
            Delete view
          </Button>
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

      <div className="grid cols-4" style={{ marginBottom: 20 }}>
        <div className="stat-card">
          <span className="stat-label">Fields</span>
          <span className="stat-value">{view.viewFields.length}</span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Filters</span>
          <span className="stat-value">{view.viewFilters.length}</span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Sorts</span>
          <span className="stat-value">{view.viewSorts.length}</span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Groups</span>
          <span className="stat-value">{view.viewGroups.length}</span>
        </div>
      </div>

      <div className="card" style={{ marginBottom: 20 }}>
        <div className="card-header">
          <h2>Fields</h2>
          <Badge>Toggle visibility</Badge>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Field</th>
                <th>Type</th>
                <th>Position</th>
                <th style={{ textAlign: "right" }}>Visible</th>
              </tr>
            </thead>
            <tbody>
              {view.viewFields.map((viewField) => {
                const field = fieldById.get(viewField.fieldMetadataId);
                return (
                  <tr key={viewField.id}>
                    <td>
                      <div className="cell-title">{field?.label ?? viewField.fieldMetadataId}</div>
                      <div className="cell-sub mono">{field?.name}</div>
                    </td>
                    <td>{field ? <Badge>{field.type}</Badge> : "—"}</td>
                    <td className="muted">{viewField.position}</td>
                    <td style={{ textAlign: "right" }}>
                      <label className="checkbox-row" style={{ justifyContent: "flex-end" }}>
                        <input
                          type="checkbox"
                          checked={viewField.isVisible}
                          disabled={busyField === viewField.id}
                          onChange={(event) =>
                            void toggleVisible(viewField.id, event.target.checked)
                          }
                        />
                        {busyField === viewField.id ? <Spinner /> : null}
                      </label>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      <div className="grid cols-2">
        <div className="card">
          <div className="card-header">
            <h2>Filters</h2>
          </div>
          {view.viewFilters.length === 0 ? (
            <div className="notice" style={{ margin: 16 }}>
              No filters. Advanced filter editing is available through the GraphQL console.
            </div>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>Field</th>
                    <th>Operand</th>
                    <th>Value</th>
                  </tr>
                </thead>
                <tbody>
                  {view.viewFilters.map((filter) => (
                    <tr key={filter.id}>
                      <td>{fieldById.get(filter.fieldMetadataId)?.label ?? filter.fieldMetadataId}</td>
                      <td>
                        <Badge>{filter.operand}</Badge>
                      </td>
                      <td className="mono small">{JSON.stringify(filter.value)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div className="card">
          <div className="card-header">
            <h2>Sorts & groups</h2>
          </div>
          <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {view.viewSorts.length === 0 && view.viewGroups.length === 0 ? (
              <div className="notice">No sorts or groups configured.</div>
            ) : null}
            {view.viewSorts.map((sort) => (
              <div className="relation-pill" key={sort.id}>
                <Icon name="jobs" size={13} />
                {fieldById.get(sort.fieldMetadataId)?.label ?? sort.fieldMetadataId} · {sort.direction}
              </div>
            ))}
            {view.viewGroups.map((group) => (
              <div className="relation-pill" key={group.id}>
                <Icon name="view" size={13} />
                {group.fieldValue}
              </div>
            ))}
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={deleteOpen}
        title="Delete view?"
        description="The view and its saved fields, filters, sorts and groups are removed."
        busy={deleting}
        onClose={() => setDeleteOpen(false)}
        onConfirm={remove}
      />
    </>
  );
}
