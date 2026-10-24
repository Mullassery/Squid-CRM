"use client";

import { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ObjectMetadata, ViewRecord } from "@/lib/types";
import {
  Alert,
  Badge,
  Button,
  EmptyState,
  Field,
  LoadingBlock,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

export default function ViewsPage() {
  const router = useRouter();
  const objects = useAsyncData(
    () =>
      gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(queries.objects, {
        first: 100,
      }),
    [],
  );
  const [objectFilter, setObjectFilter] = useState("");
  const views = useAsyncData(
    () =>
      gql<{ getViews: ViewRecord[] }>(queries.views, {
        objectMetadataId: objectFilter || null,
      }),
    [objectFilter],
  );
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [objectId, setObjectId] = useState("");
  const [type, setType] = useState("TABLE");
  const [icon, setIcon] = useState("IconList");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  if (objects.loading) return <LoadingBlock label="Loading views…" />;

  const objectList = objects.data?.objects.edges.map((edge) => edge.node) ?? [];
  const viewList = views.data?.getViews ?? [];
  const objectById = new Map(objectList.map((object) => [object.id, object]));

  async function create() {
    if (!name.trim() || !objectId) {
      setError("A name and object are required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await gql<{ createView: { id: string; name: string } }>(
        mutations.createView,
        {
          input: {
            name: name.trim(),
            objectMetadataId: objectId,
            viewType: type,
            icon,
            position: 0,
          },
        },
      );
      setNotice(`View “${result.createView.name}” created.`);
      setOpen(false);
      setName("");
      views.reload();
      router.push(`/settings/views/${result.createView.id}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create view.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Views"
        description="Saved table, kanban and calendar presentations with fields, filters, sorts and groups."
        actions={
          <Button variant="primary" size="sm" onClick={() => setOpen(true)}>
            <Icon name="plus" size={15} />
            New view
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

      <div style={{ display: "flex", gap: 10, marginBottom: 16, maxWidth: 420 }}>
        <select
          className="select"
          value={objectFilter}
          onChange={(event) => setObjectFilter(event.target.value)}
        >
          <option value="">All objects</option>
          {objectList.map((object) => (
            <option key={object.id} value={object.id}>
              {object.labelPlural}
            </option>
          ))}
        </select>
      </div>

      {views.loading ? (
        <LoadingBlock label="Loading views…" />
      ) : viewList.length === 0 ? (
        <EmptyState
          icon="view"
          title="No views"
          description="Create a view to control how records are presented."
          action={
            <Button variant="primary" onClick={() => setOpen(true)}>
              <Icon name="plus" size={15} />
              New view
            </Button>
          }
        />
      ) : (
        <div className="grid cols-2">
          {viewList.map((view) => {
            const object = objectById.get(view.objectMetadataId);
            return (
              <Link className="card" key={view.id} href={`/settings/views/${view.id}`}>
                <div className="card-body">
                  <div className="split-header" style={{ justifyContent: "space-between" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <span className="avatar sm">{view.name.slice(0, 1).toUpperCase()}</span>
                      <div>
                        <h3>{view.name}</h3>
                        <div className="faint small">
                          {object?.labelPlural ?? "Unknown object"} · {view.type}
                        </div>
                      </div>
                    </div>
                    <Badge tone={view.isCustom ? "accent" : undefined}>
                      {view.isCustom ? "Custom" : "System"}
                    </Badge>
                  </div>
                  <div className="inline-actions" style={{ marginTop: 12 }}>
                    <Badge>{view.viewFields.length} fields</Badge>
                    <Badge>{view.viewFilters.length} filters</Badge>
                    <Badge>{view.viewSorts.length} sorts</Badge>
                    <Badge>{view.viewGroups.length} groups</Badge>
                  </div>
                </div>
              </Link>
            );
          })}
        </div>
      )}

      <Modal
        open={open}
        title="New view"
        onClose={() => setOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={create} disabled={busy}>
              {busy ? <Spinner /> : null}
              Create view
            </Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label="Name" required>
            <input
              className="input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="All people"
            />
          </Field>
          <Field label="Object" required>
            <select
              className="select"
              value={objectId}
              onChange={(event) => setObjectId(event.target.value)}
            >
              <option value="">Select object…</option>
              {objectList.map((object) => (
                <option key={object.id} value={object.id}>
                  {object.labelPlural}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Type">
            <select className="select" value={type} onChange={(event) => setType(event.target.value)}>
              <option value="TABLE">Table</option>
              <option value="KANBAN">Kanban</option>
              <option value="LIST">List</option>
              <option value="CALENDAR">Calendar</option>
            </select>
          </Field>
          <Field label="Icon">
            <input className="input" value={icon} onChange={(event) => setIcon(event.target.value)} />
          </Field>
        </div>
      </Modal>
    </>
  );
}
