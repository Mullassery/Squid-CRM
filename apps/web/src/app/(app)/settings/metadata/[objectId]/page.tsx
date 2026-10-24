"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { FieldMetadata, IndexMetadata, ObjectMetadata } from "@/lib/types";
import {
  Alert,
  Badge,
  Button,
  ConfirmDialog,
  Field,
  IconButton,
  LoadingBlock,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const fieldTypes = [
  "TEXT",
  "NUMBER",
  "NUMERIC",
  "BOOLEAN",
  "DATE",
  "DATE_TIME",
  "SELECT",
  "MULTI_SELECT",
  "RATING",
  "CURRENCY",
  "ADDRESS",
  "LINKS",
  "EMAILS",
  "PHONES",
  "FULL_NAME",
  "RICH_TEXT",
  "RAW_JSON",
  "UUID",
  "ARRAY",
  "FILES",
];

interface ObjectForm {
  labelSingular: string;
  labelPlural: string;
  description: string;
  icon: string;
  color: string;
  isActive: boolean;
  isSearchable: boolean;
}

interface FieldForm {
  type: string;
  name: string;
  label: string;
  description: string;
  isNullable: boolean;
  isUnique: boolean;
  isSearchable: boolean;
  isActive: boolean;
}

interface IndexForm {
  indexType: string;
  fieldIds: Set<string>;
}

function objectForm(object: ObjectMetadata): ObjectForm {
  return {
    labelSingular: object.labelSingular,
    labelPlural: object.labelPlural,
    description: object.description ?? "",
    icon: object.icon ?? "",
    color: object.color ?? "",
    isActive: object.isActive,
    isSearchable: object.isSearchable,
  };
}

function fieldForm(field?: FieldMetadata): FieldForm {
  return {
    type: field?.type ?? "TEXT",
    name: field?.name ?? "",
    label: field?.label ?? "",
    description: field?.description ?? "",
    isNullable: field?.isNullable ?? true,
    isUnique: field?.isUnique ?? false,
    isSearchable: field?.isSearchable ?? false,
    isActive: field?.isActive ?? true,
  };
}

export default function MetadataObjectPage() {
  const params = useParams<{ objectId: string }>();
  const router = useRouter();
  const objectState = useAsyncData(
    () => gql<{ object: ObjectMetadata }>(queries.object, { id: params.objectId }),
    [params.objectId],
  );
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [objectEditing, setObjectEditing] = useState(false);
  const [objectDraft, setObjectDraft] = useState<ObjectForm | null>(null);
  const [objectBusy, setObjectBusy] = useState(false);

  const [fieldEditing, setFieldEditing] = useState<FieldMetadata | "new" | null>(null);
  const [fieldDraft, setFieldDraft] = useState<FieldForm>(fieldForm());
  const [fieldBusy, setFieldBusy] = useState(false);
  const [fieldDelete, setFieldDelete] = useState<FieldMetadata | null>(null);

  const [indexOpen, setIndexOpen] = useState(false);
  const [indexDraft, setIndexDraft] = useState<IndexForm>({ indexType: "BTREE", fieldIds: new Set() });
  const [indexBusy, setIndexBusy] = useState(false);
  const [indexDelete, setIndexDelete] = useState<IndexMetadata | null>(null);
  const [objectDelete, setObjectDelete] = useState(false);

  if (objectState.loading) return <LoadingBlock label="Loading object…" />;
  if (objectState.error || !objectState.data) {
    return (
      <div className="card">
        <div className="empty-state">
          <h3>Object not found</h3>
          <Link className="btn btn-primary" href="/settings/metadata">
            Back to data model
          </Link>
        </div>
      </div>
    );
  }

  const object = objectState.data.object;
  const fields = object.fieldsList;
  const indexes = object.indexMetadataList;

  function openObjectEdit() {
    setObjectDraft(objectForm(object));
    setObjectEditing(true);
    setError(null);
  }

  async function saveObject() {
    if (!objectDraft) return;
    setObjectBusy(true);
    setError(null);
    try {
      await gql(mutations.updateOneObject, {
        input: {
          id: object.id,
          update: {
            labelSingular: objectDraft.labelSingular.trim(),
            labelPlural: objectDraft.labelPlural.trim(),
            description: objectDraft.description.trim() || null,
            icon: objectDraft.icon.trim() || null,
            color: objectDraft.color.trim() || null,
            isActive: objectDraft.isActive,
            isSearchable: objectDraft.isSearchable,
          },
        },
      });
      setNotice("Object updated.");
      setObjectEditing(false);
      objectState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update object.");
    } finally {
      setObjectBusy(false);
    }
  }

  function openField(field?: FieldMetadata) {
    setFieldDraft(fieldForm(field));
    setFieldEditing(field ?? "new");
    setError(null);
  }

  async function saveField() {
    if (!fieldDraft.name.trim() || !fieldDraft.label.trim()) {
      setError("Field name and label are required.");
      return;
    }
    setFieldBusy(true);
    setError(null);
    try {
      if (fieldEditing === "new") {
        await gql(mutations.createOneField, {
          input: {
            field: {
              type: fieldDraft.type,
              name: fieldDraft.name.trim(),
              label: fieldDraft.label.trim(),
              description: fieldDraft.description.trim() || null,
              objectMetadataId: object.id,
              isNullable: fieldDraft.isNullable,
              isUnique: fieldDraft.isUnique,
              isSearchable: fieldDraft.isSearchable,
              isActive: fieldDraft.isActive,
            },
          },
        });
        setNotice(`Field “${fieldDraft.label}” created.`);
      } else if (fieldEditing) {
        await gql(mutations.updateOneField, {
          input: {
            id: fieldEditing.id,
            update: {
              label: fieldDraft.label.trim(),
              description: fieldDraft.description.trim() || null,
              isNullable: fieldDraft.isNullable,
              isUnique: fieldDraft.isUnique,
              isSearchable: fieldDraft.isSearchable,
              isActive: fieldDraft.isActive,
            },
          },
        });
        setNotice(`Field “${fieldDraft.label}” updated.`);
      }
      setFieldEditing(null);
      objectState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not save field.");
    } finally {
      setFieldBusy(false);
    }
  }

  async function removeField() {
    if (!fieldDelete) return;
    setFieldBusy(true);
    setError(null);
    try {
      await gql(mutations.deleteOneField, { input: { id: fieldDelete.id } });
      setNotice(`Field “${fieldDelete.label}” deleted.`);
      setFieldDelete(null);
      objectState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete field.");
    } finally {
      setFieldBusy(false);
    }
  }

  async function createIndex() {
    if (indexDraft.fieldIds.size === 0) {
      setError("Select at least one field.");
      return;
    }
    setIndexBusy(true);
    setError(null);
    try {
      await gql(mutations.createOneIndex, {
        input: {
          index: {
            objectMetadataId: object.id,
            fields: [...indexDraft.fieldIds].map((fieldMetadataId) => ({
              fieldMetadataId,
              subFieldName: null,
            })),
            indexType: indexDraft.indexType,
          },
        },
      });
      setNotice("Index created.");
      setIndexOpen(false);
      setIndexDraft({ indexType: "BTREE", fieldIds: new Set() });
      objectState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create index.");
    } finally {
      setIndexBusy(false);
    }
  }

  async function removeIndex() {
    if (!indexDelete) return;
    setIndexBusy(true);
    setError(null);
    try {
      await gql(mutations.deleteOneIndex, { input: { id: indexDelete.id } });
      setNotice("Index deleted.");
      setIndexDelete(null);
      objectState.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete index.");
    } finally {
      setIndexBusy(false);
    }
  }

  async function removeObject() {
    setObjectBusy(true);
    setError(null);
    try {
      await gql(mutations.deleteOneObject, { input: { id: object.id } });
      router.push("/settings/metadata");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete object.");
      setObjectBusy(false);
    }
  }

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <Link className="btn btn-ghost btn-sm" href="/settings/metadata">
          <Icon name="chevron-left" size={15} />
          Data model
        </Link>
      </div>
      <PageHeader
        title={object.labelPlural}
        description={object.description || `${object.nameSingular} · ${fields.length} fields · ${indexes.length} indexes`}
        actions={
          <>
            <Button variant="secondary" size="sm" onClick={openObjectEdit}>
              <Icon name="edit" size={15} />
              Edit object
            </Button>
            {!object.isSystem ? (
              <Button variant="danger-soft" size="sm" onClick={() => setObjectDelete(true)}>
                <Icon name="trash" size={15} />
                Delete
              </Button>
            ) : (
              <Badge>
                <Icon name="shield" size={12} />
                System object
              </Badge>
            )}
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

      <div className="card" style={{ marginBottom: 20 }}>
        <div className="card-header">
          <h2>Fields</h2>
          <Button size="sm" variant="primary" onClick={() => openField()}>
            <Icon name="plus" size={15} />
            New field
          </Button>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Label</th>
                <th>Name</th>
                <th>Type</th>
                <th>Relation</th>
                <th>Flags</th>
                <th style={{ textAlign: "right" }}>Actions</th>
              </tr>
            </thead>
            <tbody>
              {fields.map((field) => (
                <tr key={field.id}>
                  <td>
                    <div className="cell-title">{field.label}</div>
                    <div className="cell-sub">{field.description || "No description"}</div>
                  </td>
                  <td className="mono">{field.name}</td>
                  <td>
                    <Badge>{field.type}</Badge>
                  </td>
                  <td className="muted small">
                    {field.relation
                      ? `${field.relation.type} → ${field.relation.targetObjectMetadata.nameSingular}`
                      : "—"}
                  </td>
                  <td>
                    <div className="inline-actions">
                      {field.isSystem ? <Badge tone="accent">System</Badge> : null}
                      {field.isNullable ? null : <Badge tone="warning">Required</Badge>}
                      {field.isUnique ? <Badge tone="warning">Unique</Badge> : null}
                      {field.isSearchable ? <Badge tone="success">Searchable</Badge> : null}
                    </div>
                  </td>
                  <td>
                    <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                      <IconButton icon="edit" label="Edit field" onClick={() => openField(field)} />
                      {!field.isSystem ? (
                        <IconButton
                          icon="trash"
                          label="Delete field"
                          onClick={() => setFieldDelete(field)}
                        />
                      ) : null}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h2>Indexes</h2>
          <Button size="sm" variant="primary" onClick={() => setIndexOpen(true)}>
            <Icon name="plus" size={15} />
            New index
          </Button>
        </div>
        {indexes.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="database" size={24} />
            </span>
            <h3>No indexes</h3>
            <p>Index metadata is stored but does not create a physical PostgreSQL index yet.</p>
          </div>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Type</th>
                  <th>Fields</th>
                  <th style={{ textAlign: "right" }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {indexes.map((index) => (
                  <tr key={index.id}>
                    <td className="mono">{index.name}</td>
                    <td>
                      <Badge>{index.indexType}</Badge>
                    </td>
                    <td className="muted">
                      {index.indexFieldMetadataList
                        .map((item) => fields.find((field) => field.id === item.fieldMetadataId)?.label ?? item.fieldMetadataId)
                        .join(", ")}
                    </td>
                    <td style={{ textAlign: "right" }}>
                      <IconButton
                        icon="trash"
                        label="Delete index"
                        onClick={() => setIndexDelete(index)}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <Modal
        open={objectEditing}
        title="Edit object"
        onClose={() => setObjectEditing(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setObjectEditing(false)} disabled={objectBusy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={saveObject} disabled={objectBusy}>
              {objectBusy ? <Spinner /> : null}
              Save object
            </Button>
          </>
        }
      >
        {objectDraft ? (
          <div className="form-grid">
            <Field label="Singular label" required>
              <input
                className="input"
                value={objectDraft.labelSingular}
                onChange={(event) =>
                  setObjectDraft({ ...objectDraft, labelSingular: event.target.value })
                }
              />
            </Field>
            <Field label="Plural label" required>
              <input
                className="input"
                value={objectDraft.labelPlural}
                onChange={(event) =>
                  setObjectDraft({ ...objectDraft, labelPlural: event.target.value })
                }
              />
            </Field>
            <Field label="Description" wide>
              <input
                className="input"
                value={objectDraft.description}
                onChange={(event) =>
                  setObjectDraft({ ...objectDraft, description: event.target.value })
                }
              />
            </Field>
            <Field label="Icon">
              <input
                className="input"
                value={objectDraft.icon}
                onChange={(event) => setObjectDraft({ ...objectDraft, icon: event.target.value })}
              />
            </Field>
            <Field label="Color">
              <input
                className="input"
                value={objectDraft.color}
                onChange={(event) => setObjectDraft({ ...objectDraft, color: event.target.value })}
              />
            </Field>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={objectDraft.isActive}
                onChange={(event) =>
                  setObjectDraft({ ...objectDraft, isActive: event.target.checked })
                }
              />
              Active
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={objectDraft.isSearchable}
                onChange={(event) =>
                  setObjectDraft({ ...objectDraft, isSearchable: event.target.checked })
                }
              />
              Searchable
            </label>
          </div>
        ) : null}
      </Modal>

      <Modal
        open={fieldEditing !== null}
        title={fieldEditing === "new" ? "New field" : "Edit field"}
        onClose={() => setFieldEditing(null)}
        wide
        footer={
          <>
            <Button variant="secondary" onClick={() => setFieldEditing(null)} disabled={fieldBusy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={saveField} disabled={fieldBusy}>
              {fieldBusy ? <Spinner /> : null}
              Save field
            </Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label="Name" required help="Immutable identifier used by the API.">
            <input
              className="input"
              value={fieldDraft.name}
              disabled={fieldEditing !== "new"}
              onChange={(event) => setFieldDraft({ ...fieldDraft, name: event.target.value })}
            />
          </Field>
          <Field label="Label" required>
            <input
              className="input"
              value={fieldDraft.label}
              onChange={(event) => setFieldDraft({ ...fieldDraft, label: event.target.value })}
            />
          </Field>
          <Field label="Type" required>
            <select
              className="select"
              value={fieldDraft.type}
              disabled={fieldEditing !== "new"}
              onChange={(event) => setFieldDraft({ ...fieldDraft, type: event.target.value })}
            >
              {fieldTypes.map((type) => (
                <option key={type} value={type}>
                  {type}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Description">
            <input
              className="input"
              value={fieldDraft.description}
              onChange={(event) => setFieldDraft({ ...fieldDraft, description: event.target.value })}
            />
          </Field>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={fieldDraft.isNullable}
              onChange={(event) =>
                setFieldDraft({ ...fieldDraft, isNullable: event.target.checked })
              }
            />
            Nullable
          </label>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={fieldDraft.isUnique}
              onChange={(event) => setFieldDraft({ ...fieldDraft, isUnique: event.target.checked })}
            />
            Unique
          </label>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={fieldDraft.isSearchable}
              onChange={(event) =>
                setFieldDraft({ ...fieldDraft, isSearchable: event.target.checked })
              }
            />
            Searchable
          </label>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={fieldDraft.isActive}
              onChange={(event) => setFieldDraft({ ...fieldDraft, isActive: event.target.checked })}
            />
            Active
          </label>
        </div>
      </Modal>

      <Modal
        open={indexOpen}
        title="New index"
        onClose={() => setIndexOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setIndexOpen(false)} disabled={indexBusy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={createIndex} disabled={indexBusy}>
              {indexBusy ? <Spinner /> : null}
              Create index
            </Button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <Field label="Index type">
            <select
              className="select"
              value={indexDraft.indexType}
              onChange={(event) => setIndexDraft({ ...indexDraft, indexType: event.target.value })}
            >
              <option value="BTREE">BTREE</option>
              <option value="GIN">GIN</option>
            </select>
          </Field>
          <div>
            <div className="label" style={{ marginBottom: 8 }}>
              Fields
            </div>
            <div className="grid cols-2">
              {fields.map((field) => (
                <label className="checkbox-row" key={field.id}>
                  <input
                    type="checkbox"
                    checked={indexDraft.fieldIds.has(field.id)}
                    onChange={(event) => {
                      const next = new Set(indexDraft.fieldIds);
                      if (event.target.checked) next.add(field.id);
                      else next.delete(field.id);
                      setIndexDraft({ ...indexDraft, fieldIds: next });
                    }}
                  />
                  {field.label}
                </label>
              ))}
            </div>
          </div>
        </div>
      </Modal>

      <ConfirmDialog
        open={Boolean(fieldDelete)}
        title="Delete field?"
        description="The field metadata is removed. Existing column data may become unreachable."
        busy={fieldBusy}
        onClose={() => setFieldDelete(null)}
        onConfirm={removeField}
      />
      <ConfirmDialog
        open={Boolean(indexDelete)}
        title="Delete index?"
        description="The index metadata row is removed."
        busy={indexBusy}
        onClose={() => setIndexDelete(null)}
        onConfirm={removeIndex}
      />
      <ConfirmDialog
        open={objectDelete}
        title="Delete object?"
        description="The object metadata and its workspace table are removed."
        busy={objectBusy}
        onClose={() => setObjectDelete(false)}
        onConfirm={removeObject}
      />
    </>
  );
}
