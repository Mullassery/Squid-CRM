"use client";

import { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ObjectMetadata } from "@/lib/types";
import {
  Alert,
  Badge,
  Button,
  Field,
  LoadingBlock,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

interface NewObjectForm {
  nameSingular: string;
  namePlural: string;
  labelSingular: string;
  labelPlural: string;
  description: string;
  icon: string;
}

const empty: NewObjectForm = {
  nameSingular: "",
  namePlural: "",
  labelSingular: "",
  labelPlural: "",
  description: "",
  icon: "",
};

function titleize(value: string): string {
  return value
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase())
    .trim();
}

export default function MetadataPage() {
  const router = useRouter();
  const objects = useAsyncData(
    () =>
      gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(queries.objects, {
        first: 100,
      }),
    [],
  );
  const [open, setOpen] = useState(false);
  const [form, setForm] = useState<NewObjectForm>(empty);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  if (objects.loading) return <LoadingBlock label="Loading data model…" />;

  const list = objects.data?.objects.edges.map((edge) => edge.node) ?? [];
  const system = list.filter((object) => object.isSystem);
  const custom = list.filter((object) => !object.isSystem);

  function update(key: keyof NewObjectForm, value: string) {
    setForm((current) => {
      const next = { ...current, [key]: value };
      if (key === "nameSingular") {
        if (!current.namePlural) next.namePlural = `${value}s`;
        if (!current.labelSingular) next.labelSingular = titleize(value);
        if (!current.labelPlural) next.labelPlural = titleize(`${value}s`);
      }
      return next;
    });
  }

  async function create() {
    if (!form.nameSingular.trim() || !form.namePlural.trim() || !form.labelSingular.trim()) {
      setError("Name and label fields are required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await gql<{ createOneObject: { id: string; nameSingular: string } }>(
        mutations.createOneObject,
        {
          input: {
            object: {
              nameSingular: form.nameSingular.trim(),
              namePlural: form.namePlural.trim(),
              labelSingular: form.labelSingular.trim(),
              labelPlural: form.labelPlural.trim() || form.labelSingular.trim(),
              description: form.description.trim() || null,
              icon: form.icon.trim() || null,
              skipNameField: true,
            },
          },
        },
      );
      setNotice(`Object “${result.createOneObject.nameSingular}” created.`);
      setOpen(false);
      setForm(empty);
      router.push(`/settings/metadata/${result.createOneObject.id}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create object.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Data model"
        description="Standard and custom objects, their fields and indexes. Custom objects create physical workspace tables."
        actions={
          <Button variant="primary" size="sm" onClick={() => setOpen(true)}>
            <Icon name="plus" size={15} />
            New object
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

      <h2 style={{ marginBottom: 10 }}>Standard objects</h2>
      <div className="grid cols-2" style={{ marginBottom: 22 }}>
        {system.map((object) => (
          <Link className="card" key={object.id} href={`/settings/metadata/${object.id}`}>
            <div className="card-body">
              <div className="split-header" style={{ justifyContent: "space-between" }}>
                <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                  <span className="avatar sm">{object.labelSingular.slice(0, 1)}</span>
                  <div>
                    <h3>{object.labelPlural}</h3>
                    <div className="faint small mono">{object.nameSingular}</div>
                  </div>
                </div>
                <Badge>
                  <Icon name="shield" size={12} />
                  System
                </Badge>
              </div>
              <div className="muted small" style={{ marginTop: 12 }}>
                {object.fieldsList.length} fields · {object.indexMetadataList.length} indexes
              </div>
            </div>
          </Link>
        ))}
      </div>

      <h2 style={{ marginBottom: 10 }}>Custom objects</h2>
      {custom.length === 0 ? (
        <div className="card">
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="database" size={24} />
            </span>
            <h3>No custom objects</h3>
            <p>Create an object to store records that are specific to your business.</p>
            <Button variant="primary" onClick={() => setOpen(true)}>
              <Icon name="plus" size={15} />
              New object
            </Button>
          </div>
        </div>
      ) : (
        <div className="grid cols-2">
          {custom.map((object) => (
            <Link className="card" key={object.id} href={`/settings/metadata/${object.id}`}>
              <div className="card-body">
                <div className="split-header" style={{ justifyContent: "space-between" }}>
                  <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                    <span className="avatar sm">{object.labelSingular.slice(0, 1)}</span>
                    <div>
                      <h3>{object.labelPlural}</h3>
                      <div className="faint small mono">{object.nameSingular}</div>
                    </div>
                  </div>
                  <Badge tone="accent">Custom</Badge>
                </div>
                <div className="muted small" style={{ marginTop: 12 }}>
                  {object.fieldsList.length} fields · {object.indexMetadataList.length} indexes
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}

      <Modal
        open={open}
        title="New custom object"
        description="Object names are used for tables and identifiers; labels are shown in the UI."
        onClose={() => setOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={create} disabled={busy}>
              {busy ? <Spinner /> : null}
              Create object
            </Button>
          </>
        }
      >
        <div className="form-grid">
          {error ? (
            <div className="wide">
              <Alert tone="error">{error}</Alert>
            </div>
          ) : null}
          <Field label="Singular name" required help="Lowercase identifier, e.g. project.">
            <input
              className="input"
              value={form.nameSingular}
              onChange={(event) => update("nameSingular", event.target.value)}
            />
          </Field>
          <Field label="Plural name" required>
            <input
              className="input"
              value={form.namePlural}
              onChange={(event) => update("namePlural", event.target.value)}
            />
          </Field>
          <Field label="Singular label" required>
            <input
              className="input"
              value={form.labelSingular}
              onChange={(event) => update("labelSingular", event.target.value)}
            />
          </Field>
          <Field label="Plural label">
            <input
              className="input"
              value={form.labelPlural}
              onChange={(event) => update("labelPlural", event.target.value)}
            />
          </Field>
          <Field label="Description" wide>
            <input
              className="input"
              value={form.description}
              onChange={(event) => update("description", event.target.value)}
            />
          </Field>
          <Field label="Icon" wide>
            <input
              className="input"
              value={form.icon}
              placeholder="Optional icon name"
              onChange={(event) => update("icon", event.target.value)}
            />
          </Field>
        </div>
      </Modal>
    </>
  );
}
