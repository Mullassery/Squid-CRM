"use client";

import { gql, queries } from "@/lib/graphql";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { ObjectMetadata } from "@/lib/types";

export function useCustomObject(nameSingular: string) {
  return useAsyncData(
    async () => {
      const data = await gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(
        queries.objects,
        { first: 100 },
      );
      return (
        data.objects.edges
          .map((edge) => edge.node)
          .find(
            (object) =>
              !object.isSystem &&
              object.nameSingular.toLowerCase() === nameSingular.toLowerCase(),
          ) ?? null
      );
    },
    [nameSingular],
  );
}
