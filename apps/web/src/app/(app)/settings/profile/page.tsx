"use client";

import { useState } from "react";
import { useSession } from "@/hooks/useSession";
import { useAsyncData } from "@/hooks/useAsyncData";
import { api } from "@/lib/api";
import { gql, queries } from "@/lib/graphql";
import type { CurrentUser, JsonValue } from "@/lib/types";
import {
  Alert,
  Badge,
  Button,
  CopyButton,
  LoadingBlock,
  PageHeader,
  SectionTitle,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

export default function ProfilePage() {
  const { session, signOut } = useSession();
  const [busy, setBusy] = useState(false);
  const userState = useAsyncData(
    () => gql<{ currentUser: CurrentUser }>(queries.currentUser),
    [],
  );
  const config = useAsyncData(
    () => api.get<Record<string, JsonValue>>("/client-config"),
    [],
  );

  if (!session) return null;
  if (userState.loading) return <LoadingBlock label="Loading profile…" />;

  const user = userState.data?.currentUser;

  return (
    <>
      <PageHeader
        title="Profile"
        description="Identity, session and public client configuration."
        actions={
          <Button
            variant="danger-soft"
            size="sm"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void signOut().finally(() => setBusy(false));
            }}
          >
            {busy ? <Spinner /> : <Icon name="logout" size={15} />}
            Sign out
          </Button>
        }
      />

      <div className="card" style={{ marginBottom: 20 }}>
        <div className="card-header">
          <h2>Account</h2>
          <Badge tone="accent">{session.role}</Badge>
        </div>
        <div className="card-body">
          <div className="detail-list">
            <div className="detail-item">
              <div className="detail-label">Name</div>
              <div className="detail-value">
                {[user?.firstName, user?.lastName].filter(Boolean).join(" ") || "—"}
              </div>
            </div>
            <div className="detail-item">
              <div className="detail-label">Email</div>
              <div className="detail-value">{session.user.email}</div>
            </div>
            <div className="detail-item">
              <div className="detail-label">User ID</div>
              <div className="copy-row">
                <span className="mono">{session.user.id}</span>
                <CopyButton value={session.user.id} />
              </div>
            </div>
            <div className="detail-item">
              <div className="detail-label">Workspace</div>
              <div className="detail-value">
                {session.workspace.displayName} · <span className="mono">{session.workspace.subdomain}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <SectionTitle title="Session" />
      <div className="card" style={{ marginBottom: 20 }}>
        <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <Alert tone="warning">
            The session is an HttpOnly cookie (<span className="mono">squid-session</span>) and is
            not readable from JavaScript. Use sign out to revoke it.
          </Alert>
          <div className="notice">
            <Icon name="shield" size={14} />
            Access tokens are only used by server-side clients on <span className="mono">/metadata</span>.
          </div>
        </div>
      </div>

      <SectionTitle title="Client configuration" />
      <div className="card">
        <div className="card-body">
          {config.loading ? (
            <LoadingBlock label="Loading configuration…" />
          ) : config.data ? (
            <pre className="json-view" style={{ maxHeight: 360 }}>
              {JSON.stringify(config.data, null, 2)}
            </pre>
          ) : (
            <div className="notice">Client configuration is unavailable.</div>
          )}
        </div>
      </div>
    </>
  );
}
