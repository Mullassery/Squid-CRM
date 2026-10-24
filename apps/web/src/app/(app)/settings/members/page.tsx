"use client";

import { useState } from "react";
import { gql, mutations, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { Role, WorkspaceMember } from "@/lib/types";
import { formatDateTime, formatFullName } from "@/lib/format";
import {
  Alert,
  Avatar,
  Badge,
  LoadingBlock,
  PageHeader,
} from "@/components/ui";
import { Icon } from "@/components/icons";

export default function MembersPage() {
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const members = useAsyncData(
    () =>
      gql<{ workspaceMembers: { edges: { node: WorkspaceMember }[] } }>(
        queries.members,
        { first: 100 },
      ),
    [],
  );
  const roles = useAsyncData(() => gql<{ getRoles: Role[] }>(queries.roles), []);

  if (members.loading || roles.loading) {
    return <LoadingBlock label="Loading members…" />;
  }

  const rows = members.data?.workspaceMembers.edges.map((edge) => edge.node) ?? [];
  const roleList = roles.data?.getRoles ?? [];

  async function changeRole(member: WorkspaceMember, roleId: string) {
    setBusyId(member.id);
    setError(null);
    try {
      await gql(mutations.updateMemberRole, {
        workspaceMemberId: member.id,
        roleId,
      });
      setNotice(`${formatFullName(member.name)} now has a new role.`);
      members.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update role.");
    } finally {
      setBusyId(null);
    }
  }

  return (
    <>
      <PageHeader
        title="Members"
        description="People with access to this workspace. Member invitations are not implemented yet; this page manages existing members."
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
          <h2>{rows.length} member{rows.length === 1 ? "" : "s"}</h2>
          <Badge>
            <Icon name="shield" size={12} />
            Workspace access
          </Badge>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Member</th>
                <th>Role</th>
                <th>Joined</th>
                <th style={{ textAlign: "right" }}>Assigned role</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((member) => {
                const currentRole = member.roles?.[0];
                return (
                  <tr key={member.id}>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                        <Avatar name={member.userEmail} src={member.avatarUrl} />
                        <div>
                          <div className="cell-title">{formatFullName(member.name) || member.userEmail}</div>
                          <div className="cell-sub">{member.userEmail}</div>
                        </div>
                      </div>
                    </td>
                    <td>
                      {currentRole ? <Badge tone="accent">{currentRole.label}</Badge> : <span className="faint">—</span>}
                    </td>
                    <td className="muted">{formatDateTime(member.createdAt)}</td>
                    <td>
                      <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                        <select
                          className="select"
                          style={{ maxWidth: 220 }}
                          value={currentRole?.id ?? ""}
                          disabled={busyId === member.id}
                          onChange={(event) => {
                            if (event.target.value) void changeRole(member, event.target.value);
                          }}
                        >
                          <option value="">Select role…</option>
                          {roleList
                            .filter((role) => role.canBeAssignedToUsers)
                            .map((role) => (
                              <option key={role.id} value={role.id}>
                                {role.label}
                              </option>
                            ))}
                        </select>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      <div className="notice" style={{ marginTop: 14 }}>
        <Icon name="shield" size={14} />
        Role changes are enforced on REST and GraphQL data operations.
      </div>
    </>
  );
}
