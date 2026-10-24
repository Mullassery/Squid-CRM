"use client";

import { Suspense, useState } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { gql, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { SearchResult } from "@/lib/types";
import { Badge, Button, EmptyState, LoadingBlock, PageHeader } from "@/components/ui";
import { Icon } from "@/components/icons";

const objectToResource: Record<string, string> = {
  person: "people",
  company: "companies",
  opportunity: "opportunities",
  task: "tasks",
  note: "notes",
};

const filters = [
  { label: "All", value: "" },
  { label: "People", value: "person" },
  { label: "Companies", value: "company" },
  { label: "Opportunities", value: "opportunity" },
  { label: "Tasks", value: "task" },
  { label: "Notes", value: "note" },
];

function SearchPageInner() {
  const params = useSearchParams();
  const router = useRouter();
  const initial = params.get("q") ?? "";
  const [query, setQuery] = useState(initial);
  const [type, setType] = useState("");

  const { data, loading, error } = useAsyncData(
    async () => {
      if (!initial.trim()) return null;
      return gql<{ search: SearchResult }>(queries.search, {
        q: initial.trim(),
        limit: 30,
      });
    },
    [initial],
  );

  const results = (data?.search.edges ?? [])
    .map((edge) => edge.node)
    .filter((record) => !type || record.objectNameSingular === type);

  return (
    <>
      <PageHeader
        title="Search"
        description="One ranked search across people, companies, opportunities, tasks and notes."
      />

      <form
        className="card"
        style={{ padding: 14, marginBottom: 18, display: "flex", gap: 10 }}
        onSubmit={(event) => {
          event.preventDefault();
          const next = query.trim();
          router.push(next ? `/search?q=${encodeURIComponent(next)}` : "/search");
        }}
      >
        <input
          className="input"
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search by name, email, domain, title…"
        />
        <Button variant="primary" type="submit">
          <Icon name="search" size={15} />
          Search
        </Button>
      </form>

      <div className="tabs" style={{ marginBottom: 16 }}>
        {filters.map((filter) => (
          <button
            key={filter.value}
            type="button"
            className={`tab${type === filter.value ? " active" : ""}`}
            onClick={() => setType(filter.value)}
          >
            {filter.label}
          </button>
        ))}
      </div>

      {!initial.trim() ? (
        <EmptyState
          icon="search"
          title="Search your workspace"
          description="Try an email, company domain or record title."
        />
      ) : loading ? (
        <LoadingBlock label="Searching…" />
      ) : error ? (
        <EmptyState icon="search" title="Search failed" description={error} />
      ) : results.length === 0 ? (
        <EmptyState
          icon="search"
          title={`No matches for “${initial}”`}
          description="Try a shorter query or a different object filter."
        />
      ) : (
        <div className="card">
          <div className="card-header">
            <h2>{results.length} result{results.length === 1 ? "" : "s"}</h2>
            <span className="faint small">Ranked by PostgreSQL full-text search</span>
          </div>
          <div className="table-wrap">
            <table className="table">
              <tbody>
                {results.map((record) => {
                  const resource = objectToResource[record.objectNameSingular];
                  const href = resource
                    ? `/objects/${resource}/${record.recordId}`
                    : `/custom/${record.objectNameSingular}/${record.recordId}`;
                  return (
                    <tr key={`${record.objectNameSingular}-${record.recordId}`}>
                      <td style={{ width: 34 }}>
                        <span className="avatar sm">{record.label.slice(0, 1).toUpperCase()}</span>
                      </td>
                      <td>
                        <Link className="cell-title" href={href}>
                          {record.label}
                        </Link>
                        <div className="cell-sub mono">{record.recordId}</div>
                      </td>
                      <td style={{ textAlign: "right" }}>
                        <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                          <Badge>{record.objectLabelSingular}</Badge>
                          <span className="faint small">rank {record.tsRank.toFixed(2)}</span>
                          <Link className="icon-btn" href={href} aria-label="Open record">
                            <Icon name="chevron-right" size={16} />
                          </Link>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </>
  );
}

export default function SearchPage() {
  return (
    <Suspense fallback={<LoadingBlock label="Loading search…" />}>
      <SearchPageInner />
    </Suspense>
  );
}
