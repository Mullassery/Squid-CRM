"use client";

import { useState } from "react";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { JobRecord, JsonValue } from "@/lib/types";
import { formatDateTime } from "@/lib/format";
import {
  Alert,
  Badge,
  Button,
  Field,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const kinds = ["maintenance.prune", "webhook.replay", "http.request"];
const states = ["", "PENDING", "RUNNING", "COMPLETED", "FAILED"];

export default function JobsPage() {
  const [state, setState] = useState("");
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState(kinds[0]);
  const [payload, setPayload] = useState("{}");
  const [maxAttempts, setMaxAttempts] = useState("5");
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const jobs = useAsyncData(
    () =>
      gql<{ jobs: JobRecord[] }>(queries.jobs, { state: state || null }),
    [state],
  );

  const list = jobs.data?.jobs ?? [];

  async function enqueue() {
    let parsed: JsonValue = {};
    try {
      parsed = JSON.parse(payload) as JsonValue;
    } catch {
      setError("Payload must be valid JSON.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await gql(mutations.enqueueJob, {
        kind,
        payload: parsed,
        maxAttempts: Number(maxAttempts) || 5,
      });
      setNotice(`Job “${kind}” enqueued.`);
      setOpen(false);
      jobs.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not enqueue job.");
    } finally {
      setBusy(false);
    }
  }

  async function run(job: JobRecord) {
    setRunning(job.id);
    setError(null);
    try {
      await gql(mutations.runJob, { id: job.id });
      setNotice(`Job ${job.id.slice(0, 8)} executed.`);
      jobs.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not run job.");
    } finally {
      setRunning(null);
    }
  }

  return (
    <>
      <PageHeader
        title="Jobs"
        description="Durable workspace queue for maintenance, webhook replay and outbound HTTP requests."
        actions={
          <Button variant="primary" size="sm" onClick={() => setOpen(true)}>
            <Icon name="plus" size={15} />
            Enqueue job
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

      <div style={{ display: "flex", gap: 10, marginBottom: 16, maxWidth: 260 }}>
        <select className="select" value={state} onChange={(event) => setState(event.target.value)}>
          {states.map((value) => (
            <option key={value} value={value}>
              {value || "All states"}
            </option>
          ))}
        </select>
      </div>

      <div className="card">
        <div className="card-header">
          <h2>{list.length} job{list.length === 1 ? "" : "s"}</h2>
          <Badge>
            <Icon name="jobs" size={12} />
            Admin queue
          </Badge>
        </div>
        {jobs.loading ? (
          <div className="loading-block">
            <Spinner />
            Loading jobs…
          </div>
        ) : list.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="jobs" size={24} />
            </span>
            <h3>No jobs</h3>
            <p>Enqueue a maintenance or HTTP job to see it here.</p>
          </div>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Kind</th>
                  <th>State</th>
                  <th>Attempts</th>
                  <th>Run at</th>
                  <th>Payload</th>
                  <th style={{ textAlign: "right" }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {list.map((job) => (
                  <tr key={job.id}>
                    <td>
                      <div className="cell-title mono">{job.kind}</div>
                      <div className="cell-sub mono">{job.id.slice(0, 8)}…</div>
                    </td>
                    <td>
                      <Badge
                        tone={
                          job.state === "COMPLETED"
                            ? "success"
                            : job.state === "FAILED"
                              ? "danger"
                              : "warning"
                        }
                      >
                        {job.state}
                      </Badge>
                    </td>
                    <td>
                      {job.attempts}/{job.maxAttempts}
                    </td>
                    <td className="muted">{formatDateTime(job.runAt)}</td>
                    <td>
                      <code className="small">{JSON.stringify(job.payload).slice(0, 60)}</code>
                    </td>
                    <td style={{ textAlign: "right" }}>
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={running === job.id || job.state === "COMPLETED"}
                        onClick={() => void run(job)}
                      >
                        {running === job.id ? <Spinner /> : <Icon name="play" size={14} />}
                        Run
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <Modal
        open={open}
        title="Enqueue job"
        onClose={() => setOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={enqueue} disabled={busy}>
              {busy ? <Spinner /> : null}
              Enqueue
            </Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label="Kind" required>
            <select className="select" value={kind} onChange={(event) => setKind(event.target.value)}>
              {kinds.map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Max attempts">
            <input
              className="input"
              type="number"
              min={1}
              max={20}
              value={maxAttempts}
              onChange={(event) => setMaxAttempts(event.target.value)}
            />
          </Field>
          <Field label="Payload (JSON)" wide>
            <textarea
              className="textarea mono"
              value={payload}
              onChange={(event) => setPayload(event.target.value)}
              style={{ minHeight: 160 }}
            />
          </Field>
        </div>
      </Modal>
    </>
  );
}
