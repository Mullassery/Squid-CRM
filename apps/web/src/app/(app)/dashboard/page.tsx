"use client";

import Link from "next/link";
import { api, listKey } from "@/lib/api";
import { useAsyncData } from "@/hooks/useAsyncData";
import { useSession } from "@/hooks/useSession";
import type { FindManyResponse } from "@/lib/types";
import type { GenericRecord } from "@/lib/resources";
import { formatCurrency, formatDate, formatFullName, humanize } from "@/lib/format";
import { Alert, EmptyState, LoadingBlock, PageHeader, StatusBadge } from "@/components/ui";
import { Icon } from "@/components/icons";

interface DashboardData {
  people: GenericRecord[];
  peopleCount: number;
  companies: GenericRecord[];
  companiesCount: number;
  opportunities: GenericRecord[];
  opportunitiesCount: number;
  tasks: GenericRecord[];
  tasksCount: number;
  notes: GenericRecord[];
  notesCount: number;
  pipelineMicros: number;
  openOpportunities: number;
}

function nameOf(record: GenericRecord): string {
  const name = record.name;
  if (name && typeof name === "object") {
    return formatFullName(name as { firstName?: string; lastName?: string });
  }
  return String(record.name ?? record.title ?? "Untitled");
}

export default function DashboardPage() {
  const { session } = useSession();

  const { data, error, loading, reload } = useAsyncData<DashboardData>(async () => {
    const [people, companies, opportunities, tasks, notes] = await Promise.all([
      api.get<FindManyResponse<GenericRecord>>("/rest/people", {
        limit: 5,
        order_by: "createdAt[DescNullsLast]",
      }),
      api.get<FindManyResponse<GenericRecord>>("/rest/companies", {
        limit: 5,
        order_by: "createdAt[DescNullsLast]",
      }),
      api.get<FindManyResponse<GenericRecord>>("/rest/opportunities", {
        limit: 100,
        order_by: "createdAt[DescNullsLast]",
      }),
      api.get<FindManyResponse<GenericRecord>>("/rest/tasks", {
        limit: 5,
        order_by: "createdAt[DescNullsLast]",
      }),
      api.get<FindManyResponse<GenericRecord>>("/rest/notes", {
        limit: 5,
        order_by: "createdAt[DescNullsLast]",
      }),
    ]);

    const opportunityRows = listKey(opportunities, "opportunities");
    let pipelineMicros = 0;
    let openOpportunities = 0;
    for (const opportunity of opportunityRows) {
      const amount = opportunity.amount as { amountMicros?: number | null } | undefined;
      if (opportunity.stage !== "CUSTOMER") {
        openOpportunities += 1;
        pipelineMicros += amount?.amountMicros ?? 0;
      }
    }

    return {
      people: listKey(people, "people"),
      peopleCount: people.totalCount,
      companies: listKey(companies, "companies"),
      companiesCount: companies.totalCount,
      opportunities: opportunityRows.slice(0, 5),
      opportunitiesCount: opportunities.totalCount,
      tasks: listKey(tasks, "tasks"),
      tasksCount: tasks.totalCount,
      notes: listKey(notes, "notes"),
      notesCount: notes.totalCount,
      pipelineMicros,
      openOpportunities,
    };
  }, []);

  if (loading) return <LoadingBlock label="Loading dashboard…" />;

  if (error || !data) {
    return (
      <EmptyState
        icon="dashboard"
        title="Could not load the dashboard"
        description={error ?? undefined}
        action={
          <button className="btn btn-primary" onClick={reload}>
            Try again
          </button>
        }
      />
    );
  }

  const stats = [
    {
      label: "People",
      value: data.peopleCount,
      hint: "Contacts in this workspace",
      href: "/objects/people",
      accent: "#2563eb",
      icon: "users" as const,
    },
    {
      label: "Companies",
      value: data.companiesCount,
      hint: "Accounts and domains",
      href: "/objects/companies",
      accent: "#0f766e",
      icon: "building" as const,
    },
    {
      label: "Opportunities",
      value: data.opportunitiesCount,
      hint: `${data.openOpportunities} open`,
      href: "/objects/opportunities",
      accent: "#b45309",
      icon: "target" as const,
    },
    {
      label: "Open pipeline",
      value: formatCurrency({ amountMicros: data.pipelineMicros, currencyCode: "USD" }),
      hint: "Excludes customer-stage deals",
      href: "/objects/opportunities",
      accent: "#7c3aed",
      icon: "spark" as const,
    },
    {
      label: "Tasks",
      value: data.tasksCount,
      hint: "Work in progress",
      href: "/objects/tasks",
      accent: "#be123c",
      icon: "check" as const,
    },
    {
      label: "Notes",
      value: data.notesCount,
      hint: "Context and history",
      href: "/objects/notes",
      accent: "#0369a1",
      icon: "note" as const,
    },
  ];

  return (
    <>
      <PageHeader
        title={`Good to see you, ${session?.user.email.split("@")[0] ?? "there"}`}
        description={`${session?.workspace.displayName ?? "Workspace"} · signed in as ${session?.role.toLowerCase() ?? "member"}.`}
        actions={
          <>
            <Link className="btn btn-secondary btn-sm" href="/search">
              <Icon name="search" size={15} />
              Search
            </Link>
            <Link className="btn btn-primary btn-sm" href="/objects/people/new">
              <Icon name="plus" size={15} />
              New person
            </Link>
          </>
        }
      />

      {error ? <Alert tone="error">{error}</Alert> : null}

      <div className="grid cols-3" style={{ marginBottom: 20 }}>
        {stats.map((stat) => (
          <Link
            key={stat.label}
            href={stat.href}
            className="stat-card"
            style={{ "--stat-accent": stat.accent } as React.CSSProperties}
          >
            <div className="split-header" style={{ justifyContent: "space-between" }}>
              <span className="stat-label">{stat.label}</span>
              <span style={{ color: stat.accent }}>
                <Icon name={stat.icon} size={17} />
              </span>
            </div>
            <span className="stat-value">{stat.value}</span>
            <span className="stat-hint">{stat.hint}</span>
          </Link>
        ))}
      </div>

      <div className="grid cols-2">
        <div className="card">
          <div className="card-header">
            <h2>Recent people</h2>
            <Link className="btn btn-ghost btn-sm" href="/objects/people">
              View all
              <Icon name="chevron-right" size={14} />
            </Link>
          </div>
          {data.people.length === 0 ? (
            <EmptyState
              icon="users"
              title="No people yet"
              description="Add a person to start building your workspace."
              action={
                <Link className="btn btn-primary" href="/objects/people/new">
                  New person
                </Link>
              }
            />
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Email</th>
                    <th>Added</th>
                  </tr>
                </thead>
                <tbody>
                  {data.people.map((person) => {
                    const emails = person.emails as { primaryEmail?: string } | undefined;
                    return (
                      <tr key={String(person.id)}>
                        <td>
                          <Link className="cell-title" href={`/objects/people/${String(person.id)}`}>
                            {nameOf(person)}
                          </Link>
                        </td>
                        <td className="muted">{emails?.primaryEmail ?? "—"}</td>
                        <td className="muted">{formatDate(String(person.createdAt))}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div className="card">
          <div className="card-header">
            <h2>Recent opportunities</h2>
            <Link className="btn btn-ghost btn-sm" href="/objects/opportunities">
              View all
              <Icon name="chevron-right" size={14} />
            </Link>
          </div>
          {data.opportunities.length === 0 ? (
            <EmptyState
              icon="target"
              title="No opportunities yet"
              description="Create a deal to track pipeline."
              action={
                <Link className="btn btn-primary" href="/objects/opportunities/new">
                  New opportunity
                </Link>
              }
            />
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Stage</th>
                    <th>Amount</th>
                  </tr>
                </thead>
                <tbody>
                  {data.opportunities.map((opportunity) => (
                    <tr key={String(opportunity.id)}>
                      <td>
                        <Link className="cell-title" href={`/objects/opportunities/${String(opportunity.id)}`}>
                          {nameOf(opportunity)}
                        </Link>
                      </td>
                      <td>
                        <StatusBadge value={String(opportunity.stage)} />
                      </td>
                      <td>{formatCurrency(opportunity.amount as never)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>

      <div className="grid cols-2" style={{ marginTop: 20 }}>
        <div className="card">
          <div className="card-header">
            <h2>Open tasks</h2>
            <Link className="btn btn-ghost btn-sm" href="/objects/tasks">
              View all
              <Icon name="chevron-right" size={14} />
            </Link>
          </div>
          {data.tasks.length === 0 ? (
            <EmptyState icon="check" title="No tasks" description="Tasks you create appear here." />
          ) : (
            <div className="table-wrap">
              <table className="table">
                <tbody>
                  {data.tasks.map((task) => (
                    <tr key={String(task.id)}>
                      <td>
                        <Link className="cell-title" href={`/objects/tasks/${String(task.id)}`}>
                          {String(task.title ?? "Untitled task")}
                        </Link>
                      </td>
                      <td style={{ textAlign: "right" }}>
                        <StatusBadge value={String(task.status ?? "TODO")} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div className="card">
          <div className="card-header">
            <h2>Recent notes</h2>
            <Link className="btn btn-ghost btn-sm" href="/objects/notes">
              View all
              <Icon name="chevron-right" size={14} />
            </Link>
          </div>
          {data.notes.length === 0 ? (
            <EmptyState icon="note" title="No notes" description="Notes keep context close to the record." />
          ) : (
            <div className="table-wrap">
              <table className="table">
                <tbody>
                  {data.notes.map((note) => (
                    <tr key={String(note.id)}>
                      <td>
                        <Link className="cell-title" href={`/objects/notes/${String(note.id)}`}>
                          {String(note.title ?? "Untitled note")}
                        </Link>
                        <div className="cell-sub">{humanize(String(note.id).slice(0, 8))}</div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
