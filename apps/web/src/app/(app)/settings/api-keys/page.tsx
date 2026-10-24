"use client";

import { useState } from "react";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ApiKeyRecord, Role } from "@/lib/types";
import { formatDateTime, toDateInput } from "@/lib/format";
import {
  Alert,
  Badge,
  Button,
  CopyButton,
  Field,
  IconButton,
  LoadingBlock,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

function defaultExpiry(): string {
  const date = new Date();
  date.setDate(date.getDate() + 90);
  return date.toISOString().slice(0, 10);
}

export default function ApiKeysPage() {
  const keys = useAsyncData(() => gql<{ apiKeys: ApiKeyRecord[] }>(queries.apiKeys), []);
  const roles = useAsyncData(() => gql<{ getApiKeyRoles: Role[] }>(queries.apiKeyRoles), []);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [expiresAt, setExpiresAt] = useState(defaultExpiry);
  const [roleId, setRoleId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [token, setToken] = useState<{ label: string; token: string } | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);

  if (keys.loading || roles.loading) return <LoadingBlock label="Loading API keys…" />;

  const keyList = keys.data?.apiKeys ?? [];
  const roleList = roles.data?.getApiKeyRoles ?? [];

  async function createKey() {
    if (!name.trim() || !roleId) {
      setError("A name and role are required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const created = await gql<{ createApiKey: { id: string; name: string } }>(
        mutations.createApiKey,
        { input: { name: name.trim(), expiresAt, roleId } },
      );
      const generated = await gql<{ generateApiKeyToken: { token: string } }>(
        mutations.generateApiKeyToken,
        { id: created.createApiKey.id, expiresAt },
      );
      setToken({ label: created.createApiKey.name, token: generated.generateApiKeyToken.token });
      setNotice(`API key “${created.createApiKey.name}” created.`);
      setCreateOpen(false);
      setName("");
      setExpiresAt(defaultExpiry());
      keys.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create API key.");
    } finally {
      setBusy(false);
    }
  }

  async function generate(key: ApiKeyRecord) {
    setBusyKey(key.id);
    setError(null);
    try {
      const generated = await gql<{ generateApiKeyToken: { token: string } }>(
        mutations.generateApiKeyToken,
        { id: key.id, expiresAt: toDateInput(key.expiresAt) },
      );
      setToken({ label: key.name, token: generated.generateApiKeyToken.token });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not generate token.");
    } finally {
      setBusyKey(null);
    }
  }

  async function revoke(key: ApiKeyRecord) {
    setBusyKey(key.id);
    setError(null);
    try {
      await gql(mutations.revokeApiKey, { input: { id: key.id } });
      setNotice(`API key “${key.name}” revoked.`);
      keys.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not revoke API key.");
    } finally {
      setBusyKey(null);
    }
  }

  async function assignRole(key: ApiKeyRecord, nextRoleId: string) {
    if (!nextRoleId) return;
    setBusyKey(key.id);
    setError(null);
    try {
      await gql(mutations.assignRoleToApiKey, { apiKeyId: key.id, roleId: nextRoleId });
      setNotice(`Role updated for “${key.name}”.`);
      keys.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not assign role.");
    } finally {
      setBusyKey(null);
    }
  }

  return (
    <>
      <PageHeader
        title="API keys"
        description="Machine credentials for the REST and GraphQL data APIs. Tokens are shown once."
        actions={
          <Button variant="primary" size="sm" onClick={() => setCreateOpen(true)}>
            <Icon name="plus" size={15} />
            New API key
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

      <div className="card">
        <div className="card-header">
          <h2>{keyList.length} API key{keyList.length === 1 ? "" : "s"}</h2>
          <Badge>
            <Icon name="key" size={12} />
            Bearer api_…
          </Badge>
        </div>
        {keyList.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="key" size={24} />
            </span>
            <h3>No API keys</h3>
            <p>Create a key for scripts, sync jobs and integrations.</p>
          </div>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Role</th>
                  <th>Expires</th>
                  <th>Status</th>
                  <th style={{ textAlign: "right" }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {keyList.map((key) => {
                  const revoked = Boolean(key.revokedAt);
                  return (
                    <tr key={key.id}>
                      <td>
                        <div className="cell-title">{key.name}</div>
                        <div className="cell-sub mono">{key.id.slice(0, 8)}…</div>
                      </td>
                      <td>
                        <select
                          className="select"
                          style={{ maxWidth: 190 }}
                          value={key.role?.id ?? ""}
                          disabled={revoked || busyKey === key.id}
                          onChange={(event) => void assignRole(key, event.target.value)}
                        >
                          <option value="">Unassigned</option>
                          {roleList.map((role) => (
                            <option key={role.id} value={role.id}>
                              {role.label}
                            </option>
                          ))}
                        </select>
                      </td>
                      <td className="muted">{formatDateTime(key.expiresAt)}</td>
                      <td>
                        {revoked ? (
                          <Badge tone="danger">Revoked</Badge>
                        ) : (
                          <Badge tone="success">Active</Badge>
                        )}
                      </td>
                      <td>
                        <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                          {!revoked ? (
                            <>
                              <Button
                                size="sm"
                                variant="secondary"
                                disabled={busyKey === key.id}
                                onClick={() => void generate(key)}
                              >
                                {busyKey === key.id ? <Spinner /> : <Icon name="key" size={14} />}
                                Token
                              </Button>
                              <IconButton
                                icon="trash"
                                label="Revoke API key"
                                disabled={busyKey === key.id}
                                onClick={() => void revoke(key)}
                              />
                            </>
                          ) : (
                            <span className="faint small">No actions</span>
                          )}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <Modal
        open={createOpen}
        title="New API key"
        description="The token is returned once. Store it in your secret manager."
        onClose={() => setCreateOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={createKey} disabled={busy}>
              {busy ? <Spinner /> : null}
              Create key
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
          <Field label="Name" required>
            <input
              className="input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Warehouse sync"
            />
          </Field>
          <Field label="Expires" required>
            <input
              className="input"
              type="date"
              value={expiresAt}
              onChange={(event) => setExpiresAt(event.target.value)}
            />
          </Field>
          <Field label="Role" required wide>
            <select
              className="select"
              value={roleId}
              onChange={(event) => setRoleId(event.target.value)}
            >
              <option value="">Select role…</option>
              {roleList.map((role) => (
                <option key={role.id} value={role.id}>
                  {role.label}
                </option>
              ))}
            </select>
          </Field>
        </div>
      </Modal>

      <Modal
        open={Boolean(token)}
        title="API token"
        description="Copy this token now. It cannot be retrieved again."
        onClose={() => setToken(null)}
        footer={
          <Button variant="primary" onClick={() => setToken(null)}>
            Done
          </Button>
        }
      >
        {token ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="label">{token.label}</div>
            <div className="copy-row">
              <span className="mono" style={{ flex: 1 }}>
                {token.token}
              </span>
              <CopyButton value={token.token} />
            </div>
            <Alert tone="warning">
              Use it as <span className="mono">Authorization: Bearer {token.token.slice(0, 12)}…</span>{" "}
              on <span className="mono">/rest</span> and <span className="mono">/graphql</span>.
            </Alert>
          </div>
        ) : null}
      </Modal>
    </>
  );
}
