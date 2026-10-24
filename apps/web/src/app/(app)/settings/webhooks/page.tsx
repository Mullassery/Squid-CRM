"use client";

import { useState } from "react";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { WebhookDelivery, WebhookRecord } from "@/lib/types";
import { formatDateTime } from "@/lib/format";
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

interface WebhookForm {
  targetUrl: string;
  operations: string;
  description: string;
  secret: string;
}

const emptyForm: WebhookForm = {
  targetUrl: "",
  operations: "person.created, person.updated, person.deleted",
  description: "",
  secret: "",
};

function splitOps(value: string): string[] {
  return value
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

function Deliveries({ webhook, onClose }: { webhook: WebhookRecord; onClose: () => void }) {
  const deliveries = useAsyncData(
    () =>
      gql<{ webhookDeliveries: WebhookDelivery[] }>(queries.webhookDeliveries, {
        webhookId: webhook.id,
      }),
    [webhook.id],
  );
  const [replaying, setReplaying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function replay(id: string) {
    setReplaying(id);
    setError(null);
    try {
      await gql(mutations.replayWebhookDelivery, { id });
      deliveries.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Replay failed.");
    } finally {
      setReplaying(null);
    }
  }

  return (
    <Modal open title={`${webhook.targetUrl} deliveries`} onClose={onClose} wide>
      {error ? <Alert tone="error">{error}</Alert> : null}
      {deliveries.loading ? (
        <LoadingBlock label="Loading deliveries…" />
      ) : (deliveries.data?.webhookDeliveries.length ?? 0) === 0 ? (
        <div className="notice">No deliveries recorded for this webhook.</div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Event</th>
                <th>State</th>
                <th>Attempts</th>
                <th>Last status</th>
                <th>Created</th>
                <th style={{ textAlign: "right" }}>Replay</th>
              </tr>
            </thead>
            <tbody>
              {deliveries.data?.webhookDeliveries.map((delivery) => (
                <tr key={delivery.id}>
                  <td>
                    <div className="cell-title">{delivery.event}</div>
                    <div className="cell-sub mono">{delivery.eventId.slice(0, 8)}…</div>
                  </td>
                  <td>
                    <Badge
                      tone={
                        delivery.state === "DELIVERED" || delivery.state === "SUCCEEDED"
                          ? "success"
                          : delivery.state === "FAILED"
                            ? "danger"
                            : "warning"
                      }
                    >
                      {delivery.state}
                    </Badge>
                  </td>
                  <td>{delivery.attempts}</td>
                  <td>{delivery.lastStatusCode ?? "—"}</td>
                  <td className="muted">{formatDateTime(delivery.createdAt)}</td>
                  <td style={{ textAlign: "right" }}>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={replaying === delivery.id}
                      onClick={() => void replay(delivery.id)}
                    >
                      {replaying === delivery.id ? <Spinner /> : <Icon name="refresh" size={14} />}
                      Replay
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Modal>
  );
}

export default function WebhooksPage() {
  const webhooks = useAsyncData(() => gql<{ webhooks: WebhookRecord[] }>(queries.webhooks), []);
  const [editing, setEditing] = useState<WebhookRecord | "new" | null>(null);
  const [form, setForm] = useState<WebhookForm>(emptyForm);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<WebhookRecord | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deliveryTarget, setDeliveryTarget] = useState<WebhookRecord | null>(null);

  if (webhooks.loading) return <LoadingBlock label="Loading webhooks…" />;

  const list = webhooks.data?.webhooks ?? [];

  function openCreate() {
    setForm(emptyForm);
    setEditing("new");
    setError(null);
  }

  function openEdit(webhook: WebhookRecord) {
    setForm({
      targetUrl: webhook.targetUrl,
      operations: webhook.operations.join(", "),
      description: webhook.description ?? "",
      secret: "",
    });
    setEditing(webhook);
    setError(null);
  }

  async function save() {
    if (!form.targetUrl.trim() || splitOps(form.operations).length === 0) {
      setError("Target URL and at least one operation are required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (editing === "new") {
        await gql(mutations.createWebhook, {
          input: {
            targetUrl: form.targetUrl.trim(),
            operations: splitOps(form.operations),
            description: form.description.trim() || null,
            secret: form.secret.trim() || null,
          },
        });
        setNotice("Webhook created.");
      } else if (editing) {
        await gql(mutations.updateWebhook, {
          input: {
            id: editing.id,
            update: {
              targetUrl: form.targetUrl.trim(),
              operations: splitOps(form.operations),
              description: form.description.trim() || null,
              secret: form.secret.trim() || null,
            },
          },
        });
        setNotice("Webhook updated.");
      }
      setEditing(null);
      webhooks.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not save webhook.");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleteTarget) return;
    setDeleting(true);
    setError(null);
    try {
      await gql(mutations.deleteWebhook, { id: deleteTarget.id });
      setNotice("Webhook deleted.");
      setDeleteTarget(null);
      webhooks.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not delete webhook.");
    } finally {
      setDeleting(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Webhooks"
        description="Deliver signed record events to external systems. Deliveries retry with bounded backoff."
        actions={
          <Button variant="primary" size="sm" onClick={openCreate}>
            <Icon name="plus" size={15} />
            New webhook
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
          <h2>{list.length} webhook{list.length === 1 ? "" : "s"}</h2>
          <Badge>
            <Icon name="webhook" size={12} />
            HMAC signed
          </Badge>
        </div>
        {list.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="webhook" size={24} />
            </span>
            <h3>No webhooks</h3>
            <p>Create a webhook to stream record events to another service.</p>
          </div>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Target</th>
                  <th>Operations</th>
                  <th>Updated</th>
                  <th style={{ textAlign: "right" }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {list.map((webhook) => (
                  <tr key={webhook.id}>
                    <td>
                      <div className="cell-title">{webhook.targetUrl}</div>
                      <div className="cell-sub">{webhook.description || "No description"}</div>
                    </td>
                    <td>
                      <div className="inline-actions">
                        {webhook.operations.slice(0, 3).map((operation) => (
                          <Badge key={operation}>{operation}</Badge>
                        ))}
                        {webhook.operations.length > 3 ? (
                          <Badge>+{webhook.operations.length - 3}</Badge>
                        ) : null}
                      </div>
                    </td>
                    <td className="muted">{formatDateTime(webhook.updatedAt)}</td>
                    <td>
                      <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => setDeliveryTarget(webhook)}
                        >
                          Deliveries
                        </Button>
                        <IconButton icon="edit" label="Edit webhook" onClick={() => openEdit(webhook)} />
                        <IconButton
                          icon="trash"
                          label="Delete webhook"
                          onClick={() => setDeleteTarget(webhook)}
                        />
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <Modal
        open={editing !== null}
        title={editing === "new" ? "New webhook" : "Edit webhook"}
        onClose={() => setEditing(null)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setEditing(null)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={save} disabled={busy}>
              {busy ? <Spinner /> : null}
              Save webhook
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
          <Field label="Target URL" required wide help="HTTPS endpoints are recommended.">
            <input
              className="input"
              value={form.targetUrl}
              onChange={(event) => setForm((current) => ({ ...current, targetUrl: event.target.value }))}
              placeholder="https://example.com/hooks/squid"
            />
          </Field>
          <Field
            label="Operations"
            required
            wide
            help="Comma-separated event names, for example person.created, company.updated."
          >
            <input
              className="input"
              value={form.operations}
              onChange={(event) => setForm((current) => ({ ...current, operations: event.target.value }))}
            />
          </Field>
          <Field label="Description" wide>
            <input
              className="input"
              value={form.description}
              onChange={(event) => setForm((current) => ({ ...current, description: event.target.value }))}
            />
          </Field>
          <Field label="Secret" wide help="Stored server-side and used to sign payloads.">
            <input
              className="input"
              type="password"
              value={form.secret}
              onChange={(event) => setForm((current) => ({ ...current, secret: event.target.value }))}
            />
          </Field>
        </div>
      </Modal>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="Delete webhook?"
        description="Deliveries for this webhook stop immediately."
        busy={deleting}
        onClose={() => setDeleteTarget(null)}
        onConfirm={remove}
      />

      {deliveryTarget ? (
        <Deliveries webhook={deliveryTarget} onClose={() => setDeliveryTarget(null)} />
      ) : null}
    </>
  );
}
