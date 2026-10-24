"use client";

import Link from "next/link";
import { gql, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ObjectMetadata } from "@/lib/types";
import { Badge, EmptyState, LoadingBlock, PageHeader } from "@/components/ui";
import { Icon } from "@/components/icons";

export default function CustomObjectsPage() {
  const objects = useAsyncData(
    () =>
      gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(queries.objects, {
        first: 100,
      }),
    [],
  );

  if (objects.loading) return <LoadingBlock label="Loading custom objects…" />;

  const custom = (objects.data?.objects.edges ?? [])
    .map((edge) => edge.node)
    .filter((object) => !object.isSystem);

  return (
    <>
      <PageHeader
        title="Custom objects"
        description="Business-specific objects backed by workspace tables. Records use the generic REST and JSON GraphQL APIs."
        actions={
          <Link className="btn btn-primary btn-sm" href="/settings/metadata">
            <Icon name="database" size={15} />
            Manage data model
          </Link>
        }
      />

      {custom.length === 0 ? (
        <EmptyState
          icon="database"
          title="No custom objects"
          description="Create a custom object in the data model to store records specific to your business."
          action={
            <Link className="btn btn-primary" href="/settings/metadata">
              Open data model
            </Link>
          }
        />
      ) : (
        <div className="grid cols-2">
          {custom.map((object) => (
            <div className="card" key={object.id}>
              <div className="card-header">
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span className="avatar">{object.labelSingular.slice(0, 1)}</span>
                  <div>
                    <h3>{object.labelPlural}</h3>
                    <div className="faint small mono">{object.nameSingular}</div>
                  </div>
                </div>
                <Badge tone="accent">Custom</Badge>
              </div>
              <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div className="inline-actions">
                  {object.fieldsList.slice(0, 5).map((field) => (
                    <Badge key={field.id}>{field.label}</Badge>
                  ))}
                  {object.fieldsList.length > 5 ? (
                    <Badge>+{object.fieldsList.length - 5}</Badge>
                  ) : null}
                </div>
                <div className="inline-actions">
                  <Link className="btn btn-secondary btn-sm" href={`/custom/${object.nameSingular}`}>
                    <Icon name="database" size={14} />
                    Browse records
                  </Link>
                  <Link
                    className="btn btn-ghost btn-sm"
                    href={`/settings/metadata/${object.id}`}
                  >
                    Edit schema
                  </Link>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
