"use client";

import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { api, listKey } from "@/lib/api";
import { gql, queries } from "@/lib/graphql";
import type { GenericRecord, RelationKind } from "@/lib/resources";
import type { FindManyResponse, WorkspaceMember } from "@/lib/types";

export interface LookupOption {
  value: string;
  label: string;
}

type LookupMaps = Record<RelationKind, Map<string, string>>;

interface LookupContextValue {
  options: Record<RelationKind, LookupOption[]>;
  label: (kind: RelationKind, id: string | null | undefined) => string;
  loading: boolean;
  refresh: () => void;
}

const emptyMaps = (): LookupMaps => ({
  person: new Map(),
  company: new Map(),
  opportunity: new Map(),
  task: new Map(),
  note: new Map(),
  member: new Map(),
});

const LookupContext = createContext<LookupContextValue | null>(null);

async function loadList(path: string, key: string): Promise<GenericRecord[]> {
  try {
    const response = await api.get<FindManyResponse<GenericRecord>>(path, {
      limit: 100,
    });
    return listKey(response, key);
  } catch {
    return [];
  }
}

function fullName(value: unknown): string {
  if (!value || typeof value !== "object") return "";
  const record = value as Record<string, unknown>;
  return [record.firstName, record.lastName].filter(Boolean).join(" ");
}

export function LookupProvider({ children }: { children: React.ReactNode }) {
  const [maps, setMaps] = useState<LookupMaps>(emptyMaps);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let active = true;
    setLoading(true);
    Promise.all([
      loadList("/rest/people", "people"),
      loadList("/rest/companies", "companies"),
      loadList("/rest/opportunities", "opportunities"),
      loadList("/rest/tasks", "tasks"),
      loadList("/rest/notes", "notes"),
      gql<{ workspaceMembers: { edges: { node: WorkspaceMember }[] } }>(
        queries.members,
        { first: 100 },
      ).catch(() => ({ workspaceMembers: { edges: [] } })),
    ])
      .then(([people, companies, opportunities, tasks, notes, members]) => {
        if (!active) return;
        const next = emptyMaps();
        for (const person of people) {
          const name = (person.name ?? {}) as Record<string, unknown>;
          const email = (person.emails ?? {}) as Record<string, unknown>;
          next.person.set(
            String(person.id),
            [fullName(name), email.primaryEmail].filter(Boolean).join(" · ") || String(person.id),
          );
        }
        for (const company of companies) {
          const domain = (company.domainName ?? {}) as Record<string, unknown>;
          next.company.set(
            String(company.id),
            [company.name, domain.primaryLinkUrl].filter(Boolean).join(" · ") || String(company.id),
          );
        }
        for (const opportunity of opportunities) {
          next.opportunity.set(
            String(opportunity.id),
            [opportunity.name, opportunity.stage].filter(Boolean).join(" · ") || String(opportunity.id),
          );
        }
        for (const task of tasks) {
          next.task.set(
            String(task.id),
            [task.title, task.status].filter(Boolean).join(" · ") || String(task.id),
          );
        }
        for (const note of notes) {
          next.note.set(String(note.id), String(note.title ?? note.id));
        }
        for (const edge of members.workspaceMembers.edges) {
          const member = edge.node;
          next.member.set(
            member.id,
            [fullName(member.name), member.userEmail].filter(Boolean).join(" · "),
          );
        }
        setMaps(next);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [nonce]);

  const value = useMemo<LookupContextValue>(() => {
    const options = {} as Record<RelationKind, LookupOption[]>;
    (Object.keys(maps) as RelationKind[]).forEach((kind) => {
      options[kind] = [...maps[kind].entries()].map(([value, label]) => ({
        value,
        label,
      }));
    });
    return {
      options,
      loading,
      refresh: () => setNonce((current) => current + 1),
      label: (kind, id) => {
        if (!id) return "—";
        return maps[kind].get(id) ?? id;
      },
    };
  }, [maps, loading]);

  return <LookupContext.Provider value={value}>{children}</LookupContext.Provider>;
}

export function useLookups(): LookupContextValue {
  const context = useContext(LookupContext);
  if (!context) {
    throw new Error("useLookups must be used within LookupProvider");
  }
  return context;
}
