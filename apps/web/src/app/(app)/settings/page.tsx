"use client";

import Link from "next/link";
import { useSession } from "@/hooks/useSession";
import { useAsyncData } from "@/hooks/useAsyncData";
import { gql, queries } from "@/lib/graphql";
import type { CurrentUser } from "@/lib/types";
import { Badge, CopyButton, LoadingBlock, PageHeader, SectionTitle } from "@/components/ui";
import { Icon } from "@/components/icons";

export default function SettingsGeneralPage() {
  const { session } = useSession();
  const { data, loading } = useAsyncData(
    () => gql<{ currentUser: CurrentUser }>(queries.currentUser),
    [],
  );

  if (!session) return null;
  if (loading) return <LoadingBlock label="Loading workspace…" />;

  const user = data?.currentUser;

  return (
    <>
      <PageHeader
        title="General"
        description="Workspace identity and the connection used by this web client."
      />

      <div className="card">
        <div className="card-header">
          <h2>Workspace</h2>
          <Badge tone="accent">{session.role.toLowerCase()}</Badge>
        </div>
        <div className="card-body">
          <div className="detail-list">
            <div className="detail-item">
              <div className="detail-label">Name</div>
              <div className="detail-value">
                {user?.currentWorkspace?.displayName ?? session.workspace.displayName}
              </div>
            </div>
            <div className="detail-item">
              <div className="detail-label">Subdomain</div>
              <div className="detail-value">{session.workspace.subdomain}</div>
            </div>
            <div className="detail-item">
              <div className="detail-label">Members</div>
              <div className="detail-value">
                {user?.currentWorkspace?.workspaceMembersCount ?? "—"}
              </div>
            </div>
            <div className="detail-item">
              <div className="detail-label">Workspace ID</div>
              <div className="copy-row">
                <span className="mono">{session.workspace.id}</span>
                <CopyButton value={session.workspace.id} />
              </div>
            </div>
          </div>
        </div>
      </div>

      <SectionTitle title="Signed-in user" />
      <div className="card">
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
              <div className="detail-label">Role</div>
              <div className="detail-value">{session.role}</div>
            </div>
            <div className="detail-item">
              <div className="detail-label">User ID</div>
              <div className="copy-row">
                <span className="mono">{session.user.id}</span>
                <CopyButton value={session.user.id} />
              </div>
            </div>
          </div>
        </div>
      </div>

      <SectionTitle title="Web client connection" />
      <div className="card">
        <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <p className="muted">
            The browser talks to this Next.js app, which proxies <span className="mono">/api/*</span>{" "}
            to the Rust server. Authentication uses the HttpOnly{" "}
            <span className="mono">squid-session</span> cookie.
          </p>
          <div className="code-block">/api</div>
          <div className="inline-actions">
            <Link className="btn btn-secondary btn-sm" href="/settings/api-keys">
              <Icon name="key" size={15} />
              API keys
            </Link>
            <Link className="btn btn-secondary btn-sm" href="/settings/console">
              <Icon name="terminal" size={15} />
              GraphQL console
            </Link>
          </div>
        </div>
      </div>
    </>
  );
}
