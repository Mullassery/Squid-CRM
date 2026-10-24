"use client";

import { useState } from "react";
import { gql } from "@/lib/graphql";
import {
  Alert,
  Button,
  Field,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const sample = `query {
  currentWorkspace {
    id
    displayName
    subdomain
    workspaceMembersCount
  }
}`;

export default function ConsolePage() {
  const [endpoint, setEndpoint] = useState<"/graphql" | "/metadata">("/metadata");
  const [query, setQuery] = useState(sample);
  const [variables, setVariables] = useState("{}");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function execute() {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      let parsed: Record<string, unknown> = {};
      if (variables.trim()) {
        parsed = JSON.parse(variables) as Record<string, unknown>;
      }
      const data = await gql<Record<string, unknown>>(query, parsed, endpoint);
      setResult(JSON.stringify(data, null, 2));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Request failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="API console"
        description="Run GraphQL operations against this workspace. Useful for advanced metadata, view and role changes not yet exposed as dedicated forms."
        actions={
          <Button variant="primary" size="sm" onClick={execute} disabled={busy || !query.trim()}>
            {busy ? <Spinner /> : <Icon name="play" size={15} />}
            Execute
          </Button>
        }
      />

      <div className="grid cols-2" style={{ marginBottom: 16 }}>
        <div className="card">
          <div className="card-header">
            <h2>Request</h2>
            <select
              className="select"
              style={{ maxWidth: 150 }}
              value={endpoint}
              onChange={(event) => setEndpoint(event.target.value as "/graphql" | "/metadata")}
            >
              <option value="/metadata">/metadata</option>
              <option value="/graphql">/graphql</option>
            </select>
          </div>
          <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <Field label="Query">
              <textarea
                className="textarea mono"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                style={{ minHeight: 260 }}
                spellCheck={false}
              />
            </Field>
            <Field label="Variables (JSON)">
              <textarea
                className="textarea mono"
                value={variables}
                onChange={(event) => setVariables(event.target.value)}
                style={{ minHeight: 110 }}
                spellCheck={false}
              />
            </Field>
          </div>
        </div>

        <div className="card">
          <div className="card-header">
            <h2>Response</h2>
            {result ? <span className="badge success">Success</span> : null}
          </div>
          <div className="card-body">
            {error ? <Alert tone="error">{error}</Alert> : null}
            {result ? <pre className="json-view" style={{ maxHeight: 520 }}>{result}</pre> : null}
            {!error && !result ? (
              <div className="notice">
                Run an operation to see its JSON response. Requests use the current session
                cookie and the server enforces role permissions.
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </>
  );
}
