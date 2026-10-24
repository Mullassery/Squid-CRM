"use client";

import { useState } from "react";
import { useParams } from "next/navigation";
import Link from "next/link";
import { api, downloadCsv, importCsv, listKey } from "@/lib/api";
import { resourcesByKey, type GenericRecord } from "@/lib/resources";
import { useAsyncData } from "@/hooks/useAsyncData";
import type { FindManyResponse, PageInfo } from "@/lib/types";
import { RecordTable } from "@/components/record-table";
import {
  Alert,
  Button,
  EmptyState,
  Field,
  Modal,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

const PAGE_SIZE = 25;

export default function ObjectListPage() {
  const params = useParams<{ object: string }>();
  const resource = resourcesByKey[params.object];
  const [offset, setOffset] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [csv, setCsv] = useState("");
  const [importing, setImporting] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const { data, error, loading, reload } = useAsyncData(
    async () => {
      if (!resource) {
        return { data: {}, totalCount: 0, pageInfo: undefined } as {
          data: Record<string, GenericRecord[]>;
          totalCount: number;
          pageInfo?: PageInfo;
        };
      }
      const response = await api.get<FindManyResponse<GenericRecord>>(resource.restPath, {
        limit: PAGE_SIZE,
        offset,
        order_by: resource.defaultSort,
      });
      return {
        data: response.data,
        totalCount: response.totalCount,
        pageInfo: response.pageInfo,
      };
    },
    [resource?.key, offset],
  );

  if (!resource) {
    return (
      <EmptyState
        icon="search"
        title="Object not found"
        description="This resource does not exist."
        action={
          <Link className="btn btn-primary" href="/dashboard">
            Back to dashboard
          </Link>
        }
      />
    );
  }

  const records = data ? listKey(data as FindManyResponse<GenericRecord>, resource.listKey) : [];

  async function archive(record: GenericRecord) {
    setActionError(null);
    try {
      await api.delete(`${resource.restPath}/${String(record.id)}`, { soft_delete: true });
      setNotice(`${resource.title(record)} archived.`);
      reload();
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Archive failed.");
    }
  }

  async function exportCsv() {
    setExporting(true);
    setActionError(null);
    try {
      const blob = await downloadCsv(`${resource.restPath}/export`);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${resource.key}.csv`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Export failed.");
    } finally {
      setExporting(false);
    }
  }

  async function runImport() {
    if (!csv.trim()) return;
    setImporting(true);
    setActionError(null);
    try {
      const result = await importCsv(`${resource.restPath}/import`, csv);
      setNotice(`Imported ${result.imported ?? 0} record(s).`);
      setImportOpen(false);
      setCsv("");
      reload();
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Import failed.");
    } finally {
      setImporting(false);
    }
  }

  return (
    <>
      <PageHeader
        title={resource.labelPlural}
        description={resource.description}
        actions={
          <>
            <Button size="sm" variant="secondary" onClick={exportCsv} disabled={exporting}>
              {exporting ? <Spinner /> : <Icon name="download" size={15} />}
              Export
            </Button>
            <Button size="sm" variant="secondary" onClick={() => setImportOpen(true)}>
              <Icon name="upload" size={15} />
              Import
            </Button>
            <Link className="btn btn-primary btn-sm" href={`/objects/${resource.key}/new`}>
              <Icon name="plus" size={15} />
              New {resource.label.toLowerCase()}
            </Link>
          </>
        }
      />

      {notice ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="success" onDismiss={() => setNotice(null)}>
            {notice}
          </Alert>
        </div>
      ) : null}
      {actionError ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="error" onDismiss={() => setActionError(null)}>
            {actionError}
          </Alert>
        </div>
      ) : null}

      <RecordTable
        resource={resource}
        records={records}
        loading={loading}
        error={error}
        totalCount={data?.totalCount ?? 0}
        pageInfo={data?.pageInfo}
        offset={offset}
        pageSize={PAGE_SIZE}
        onPrevious={() => setOffset((value) => Math.max(0, value - PAGE_SIZE))}
        onNext={() => setOffset((value) => value + PAGE_SIZE)}
        onArchive={archive}
      />

      <Modal
        open={importOpen}
        title={`Import ${resource.labelPlural.toLowerCase()}`}
        description="Upload a CSV with a header row. Existing records are not overwritten."
        onClose={() => setImportOpen(false)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setImportOpen(false)} disabled={importing}>
              Cancel
            </Button>
            <Button variant="primary" onClick={runImport} disabled={importing || !csv.trim()}>
              {importing ? <Spinner /> : null}
              Import
            </Button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <Field label="CSV file">
            <input
              className="input"
              type="file"
              accept=".csv,text/csv"
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (!file) return;
                void file.text().then(setCsv);
              }}
            />
          </Field>
          <Field label="Or paste CSV" wide>
            <textarea
              className="textarea"
              value={csv}
              onChange={(event) => setCsv(event.target.value)}
              placeholder="firstName,lastName,primaryEmail,jobTitle"
              style={{ minHeight: 180 }}
            />
          </Field>
          <Alert tone="warning">
            Import uses the object&apos;s documented CSV columns. Rows without a first
            name are skipped.
          </Alert>
        </div>
      </Modal>
    </>
  );
}
