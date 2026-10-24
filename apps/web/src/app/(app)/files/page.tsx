"use client";

import { useState } from "react";
import { apiUrl, uploadToSignedUrl } from "@/lib/api";
import { gql, mutations } from "@/lib/graphql";
import {
  Alert,
  Badge,
  Button,
  CopyButton,
  Field,
  PageHeader,
  Spinner,
} from "@/components/ui";
import { Icon } from "@/components/icons";

interface UploadedFile {
  id: string;
  path: string;
  url: string;
  size: number;
  contentType: string;
  name: string;
}

export default function FilesPage() {
  const [file, setFile] = useState<File | null>(null);
  const [folder, setFolder] = useState("FilesField");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [uploads, setUploads] = useState<UploadedFile[]>([]);

  async function upload() {
    if (!file) {
      setError("Choose a file first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const target = await gql<{
        createFileUpload: {
          fileId: string;
          uploadUrl: string;
          contentType: string;
        };
      }>(mutations.fileUpload, {
        filename: file.name,
        size: file.size,
        folder,
      });
      await uploadToSignedUrl(target.createFileUpload.uploadUrl, file);
      const completed = await gql<{
        completeFileUpload: { id: string; path: string; size: number; url: string };
      }>(mutations.completeFileUpload, {
        fileId: target.createFileUpload.fileId,
      });
      setUploads((current) => [
        {
          id: completed.completeFileUpload.id,
          path: completed.completeFileUpload.path,
          url: completed.completeFileUpload.url,
          size: completed.completeFileUpload.size,
          contentType: target.createFileUpload.contentType,
          name: file.name,
        },
        ...current,
      ]);
      setFile(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Upload failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Files"
        description="Upload files through signed URLs. Downloads are HMAC-signed and expire after one hour."
      />

      {error ? (
        <div style={{ marginBottom: 14 }}>
          <Alert tone="error" onDismiss={() => setError(null)}>
            {error}
          </Alert>
        </div>
      ) : null}

      <div className="grid cols-2">
        <div className="card">
          <div className="card-header">
            <h2>Upload</h2>
            <Badge>
              <Icon name="upload" size={12} />
              PUT signed URL
            </Badge>
          </div>
          <div className="card-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <Field label="File" required>
              <input
                className="input"
                type="file"
                onChange={(event) => setFile(event.target.files?.[0] ?? null)}
              />
            </Field>
            <Field label="Folder">
              <select
                className="select"
                value={folder}
                onChange={(event) => setFolder(event.target.value)}
              >
                <option value="FilesField">Files field</option>
                <option value="RecordExport">Record export</option>
                <option value="CorePicture">Core picture</option>
                <option value="EmailAttachment">Email attachment</option>
                <option value="EmailImage">Email image</option>
                <option value="Workflow">Workflow</option>
                <option value="PublicAsset">Public asset</option>
              </select>
            </Field>
            <Button variant="primary" onClick={upload} disabled={busy || !file}>
              {busy ? <Spinner /> : <Icon name="upload" size={15} />}
              Upload file
            </Button>
            <div className="notice">
              <Icon name="shield" size={14} />
              Requires the <span className="mono">UPLOAD_FILE</span> permission flag.
            </div>
          </div>
        </div>

        <div className="card">
          <div className="card-header">
            <h2>Recent uploads</h2>
            <span className="faint small">This session</span>
          </div>
          {uploads.length === 0 ? (
            <div className="empty-state">
              <span className="empty-icon">
                <Icon name="file" size={24} />
              </span>
              <h3>No uploads in this session</h3>
              <p>The API does not expose a file listing endpoint; only signed URLs returned at completion.</p>
            </div>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <tbody>
                  {uploads.map((upload) => (
                    <tr key={upload.id}>
                      <td>
                        <div className="cell-title">{upload.name}</div>
                        <div className="cell-sub mono">{upload.id.slice(0, 8)}…</div>
                      </td>
                      <td className="muted small">{upload.contentType}</td>
                      <td style={{ textAlign: "right" }}>
                        <div className="inline-actions" style={{ justifyContent: "flex-end" }}>
                          <a
                            className="btn btn-secondary btn-sm"
                            href={apiUrl(upload.url)}
                            target="_blank"
                            rel="noreferrer"
                          >
                            <Icon name="download" size={14} />
                            Open
                          </a>
                          <CopyButton value={upload.url} label="URL" />
                        </div>
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
