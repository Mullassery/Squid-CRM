"use client";

import { useState } from "react";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { PermissionFlag, Role } from "@/lib/types";
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

const capabilityFields: { key: keyof Role; label: string; hint: string }[] = [
  { key: "canUpdateAllSettings", label: "Update all settings", hint: "Workspace configuration" },
  { key: "canAccessAllTools", label: "Access all tools", hint: "API keys, webhooks and jobs" },
  { key: "canReadAllObjectRecords", label: "Read all records", hint: "Data API reads" },
  { key: "canUpdateAllObjectRecords", label: "Update all records", hint: "Create and edit" },
  { key: "canSoftDeleteAllObjectRecords", label: "Archive records", hint: "Recoverable delete" },
  { key: "canDestroyAllObjectRecords", label: "Delete permanently", hint: "Irreversible delete" },
  { key: "canBeAssignedToUsers", label: "Assign to users", hint: "Workspace members" },
  { key: "canBeAssignedToApiKeys", label: "Assign to API keys", hint: "Machine credentials" },
  { key: "canBeAssignedToAgents", label: "Assign to agents", hint: "Reserved for agents" },
];

interface RoleForm {
  label: string;
  description: string;
  icon: string;
  flags: Set<string>;
  capabilities: Record<string, boolean>;
}

function initialForm(role?: Role): RoleForm {
  return {
    label: role?.label ?? "",
    description: role?.description ?? "",
    icon: role?.icon ?? "",
    flags: new Set(role?.permissionFlags?.map((flag) => flag.flag) ?? []),
    capabilities: Object.fromEntries(
      capabilityFields.map((field) => [field.key, Boolean(role?.[field.key])]),
    ),
  };
}

export default function RolesPage() {
  const roles = useAsyncData(() => gql<{ getRoles: Role[] }>(queries.roles), []);
  const flags = useAsyncData(
    () => gql<{ getPermissionFlags: PermissionFlag[] }>(queries.permissionFlags),
    [],
  );
  const [editing, setEditing] = useState<Role | "new" | null>(null);
  const [form, setForm] = useState<RoleForm>(initialForm());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Role | null>(null);
  const [deleting, setDeleting] = useState(false);

  if (roles.loading || flags.loading) return <LoadingBlock label="Loading roles…" />;

  const roleList = roles.data?.getRoles ?? [];
  const flagList = flags.data?.getPermissionFlags ?? [];

  function openCreate() {
    setForm(initialForm());
    setEditing("new");
    setError(null);
  }

  function openEdit(role: Role) {
    setForm(initialForm(role));
    setEditing(role);
    setError(null);
  }

  function toggleCapability(key: string, value: boolean) {
    setForm((current) => ({
      ...current,
      capabilities: { ...current.capabilities, [key]: value },
    }));
  }

  function toggleFlag(key: string, value: boolean) {
    setForm((current) => {
      const flags = new Set(current.flags);
      if (value) flags.add(key);
      else flags.delete(key);
      return { ...current, flags };
    });
  }

  async function save() {
    if (!form.label.trim()) {
      setError("Role label is required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const payload = {
        label: form.label.trim(),
        description: form.description.trim() || null,
        icon: form.icon.trim() || null,
        ...Object.fromEntries(
          capabilityFields.map((field) => [field.key, Boolean(form.capabilities[field.key])]),
        ),
      };
      let roleId: string;
      if (editing === "new") {
        const created = await gql<{ createOneRole: { id: string } }>(mutations.createRole, {
          input: payload,
        });
        roleId = created.createOneRole.id;
        setNotice(`Role “${form.label.trim()}” created.`);
      } else if (editing) {
        await gql(mutations.updateRole, {
          input: { id: editing.id, update: payload },
        });
        roleId = editing.id;
        setNotice(`Role “${form.label.trim()}” updated.`);
      } else {
        return;
      }
      await gql(mutations.upsertPermissionFlags, {
        input: { roleId, permissionFlagKeys: [...form.flags] },
      });
      setEditing(null);
      roles.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not save role.");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleteTarget) return;
    setDeleting(true);
    setError(null);
    try {
      await gql(mutations.deleteRole, { roleId: deleteTarget.id });
      setNotice(`Role “${deleteTarget.label}” deleted.`);
      setDeleteTarget(null);
      roles.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete role.");
    } finally {
      setDeleting(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Roles & permissions"
        description="Built-in roles are immutable. Create custom roles and assign permission flags."
        actions={
          <Button variant="primary" size="sm" onClick={openCreate}>
            <Icon name="plus" size={15} />
            New role
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

      <div className="grid cols-2" style={{ marginBottom: 18 }}>
        {roleList.map((role) => (
          <div className="card" key={role.id}>
            <div className="card-header">
              <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
                <span className="avatar sm">{role.label.slice(0, 1).toUpperCase()}</span>
                <div>
                  <h3>{role.label}</h3>
                  <div className="faint small">{role.isEditable ? "Custom role" : "Built-in role"}</div>
                </div>
              </div>
              <div className="inline-actions">
                {role.isEditable ? (
                  <>
                    <IconButton icon="edit" label="Edit role" onClick={() => openEdit(role)} />
                    <IconButton icon="trash" label="Delete role" onClick={() => setDeleteTarget(role)} />
                  </>
                ) : (
                  <Badge>
                    <Icon name="shield" size={12} />
                    System
                  </Badge>
                )}
              </div>
            </div>
            <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <p className="muted small">{role.description || "No description."}</p>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
                {capabilityFields
                  .filter((field) => role[field.key])
                  .slice(0, 6)
                  .map((field) => (
                    <Badge key={field.key} tone="accent">
                      {field.label}
                    </Badge>
                  ))}
                {role.permissionFlags?.length ? (
                  <Badge>{role.permissionFlags.length} flags</Badge>
                ) : null}
              </div>
            </div>
          </div>
        ))}
      </div>

      <Modal
        open={editing !== null}
        onClose={() => setEditing(null)}
        title={editing === "new" ? "New role" : "Edit role"}
        description="Capabilities apply to data and settings actions. Permission flags grant settings surfaces."
        wide
        footer={
          <>
            <Button variant="secondary" onClick={() => setEditing(null)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={save} disabled={busy}>
              {busy ? <Spinner /> : null}
              Save role
            </Button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {error ? <Alert tone="error">{error}</Alert> : null}
          <div className="form-grid">
            <Field label="Label" required>
              <input
                className="input"
                value={form.label}
                onChange={(event) => setForm((current) => ({ ...current, label: event.target.value }))}
              />
            </Field>
            <Field label="Icon">
              <input
                className="input"
                value={form.icon}
                placeholder="Icon name"
                onChange={(event) => setForm((current) => ({ ...current, icon: event.target.value }))}
              />
            </Field>
            <Field label="Description" wide>
              <textarea
                className="textarea"
                value={form.description}
                onChange={(event) =>
                  setForm((current) => ({ ...current, description: event.target.value }))
                }
                style={{ minHeight: 80 }}
              />
            </Field>
          </div>

          <div>
            <div className="label" style={{ marginBottom: 8 }}>
              Capabilities
            </div>
            <div className="grid cols-2">
              {capabilityFields.map((field) => (
                <label className="checkbox-row" key={field.key}>
                  <input
                    type="checkbox"
                    checked={Boolean(form.capabilities[field.key])}
                    onChange={(event) => toggleCapability(field.key, event.target.checked)}
                  />
                  <span>
                    <span>{field.label}</span>
                    <span className="help" style={{ display: "block" }}>
                      {field.hint}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          </div>

          <div>
            <div className="label" style={{ marginBottom: 8 }}>
              Permission flags
            </div>
            <div className="grid cols-2">
              {flagList.map((flag) => (
                <label className="checkbox-row" key={flag.key}>
                  <input
                    type="checkbox"
                    checked={form.flags.has(flag.key)}
                    onChange={(event) => toggleFlag(flag.key, event.target.checked)}
                  />
                  <span>
                    <span>{flag.label}</span>
                    <span className="help" style={{ display: "block" }}>
                      {flag.key}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          </div>
        </div>
      </Modal>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="Delete role?"
        description="Members and API keys assigned to this role must be reassigned first."
        busy={deleting}
        onClose={() => setDeleteTarget(null)}
        onConfirm={remove}
      />
    </>
  );
}
