"use client";

import type { ButtonHTMLAttributes, ReactNode } from "react";
import { useEffect } from "react";
import { Icon, type IconName } from "./icons";
import { initials } from "@/lib/format";

export function cn(...values: (string | false | null | undefined)[]): string {
  return values.filter(Boolean).join(" ");
}

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "danger-soft";

export function Button({
  variant = "secondary",
  size,
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "sm";
}) {
  return (
    <button
      className={cn(
        "btn",
        `btn-${variant}`,
        size === "sm" && "btn-sm",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: IconName;
  label: string;
}) {
  return (
    <button
      className={cn("icon-btn", className)}
      aria-label={label}
      title={label}
      {...props}
    >
      <Icon name={icon} size={16} />
    </button>
  );
}

export function Badge({
  tone,
  children,
}: {
  tone?: "success" | "warning" | "danger" | "accent";
  children: ReactNode;
}) {
  return <span className={cn("badge", tone)}>{children}</span>;
}

const toneByStatus: Record<string, "success" | "warning" | "danger" | "accent"> = {
  DONE: "success",
  CUSTOMER: "success",
  ACTIVE: "success",
  IN_PROGRESS: "warning",
  PROPOSAL: "accent",
  MEETING: "accent",
  SCREENING: "accent",
  NEW: "accent",
  TODO: "warning",
  REVOKED: "danger",
  FAILED: "danger",
};

export function StatusBadge({ value }: { value: string | null | undefined }) {
  if (!value) return <span className="faint">—</span>;
  const normalized = value.toUpperCase();
  const label = normalized.replace(/_/g, " ").toLowerCase();
  return (
    <Badge tone={toneByStatus[normalized]}>
      <span className="dot" />
      {label.replace(/^\w/, (char) => char.toUpperCase())}
    </Badge>
  );
}

export function Field({
  label,
  required,
  help,
  wide,
  children,
}: {
  label: string;
  required?: boolean;
  help?: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={cn("field", wide && "wide")}>
      <span className="label">
        {label}
        {required ? <span className="req">*</span> : null}
      </span>
      {children}
      {help ? <span className="help">{help}</span> : null}
    </div>
  );
}

export function Spinner({ large }: { large?: boolean }) {
  return <span className={cn("spinner", large && "lg")} role="status" aria-label="Loading" />;
}

export function LoadingBlock({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="loading-block">
      <Spinner />
      <span>{label}</span>
    </div>
  );
}

export function Alert({
  tone,
  children,
  onDismiss,
}: {
  tone?: "error" | "success" | "warning";
  children: ReactNode;
  onDismiss?: () => void;
}) {
  return (
    <div className={cn("alert", tone)}>
      <div style={{ flex: 1 }}>{children}</div>
      {onDismiss ? <IconButton icon="close" label="Dismiss" onClick={onDismiss} /> : null}
    </div>
  );
}

export function EmptyState({
  icon = "spark",
  title,
  description,
  action,
}: {
  icon?: IconName;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <span className="empty-icon">
        <Icon name={icon} size={24} />
      </span>
      <h3>{title}</h3>
      {description ? <p>{description}</p> : null}
      {action}
    </div>
  );
}

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        <h1>{title}</h1>
        {description ? <p>{description}</p> : null}
      </div>
      {actions ? <div className="header-actions">{actions}</div> : null}
    </header>
  );
}

export function Avatar({
  name,
  size,
  src,
}: {
  name: string;
  size?: "sm" | "lg";
  src?: string | null;
}) {
  if (src) {
    return (
      <span
        className={cn("avatar", size)}
        role="img"
        aria-label={name}
        style={{ backgroundImage: `url(${src})`, backgroundSize: "cover", backgroundPosition: "center" }}
      />
    );
  }
  return <span className={cn("avatar", size)}>{initials(name)}</span>;
}

export function Modal({
  title,
  description,
  open,
  onClose,
  children,
  footer,
  wide,
}: {
  title: string;
  description?: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className={cn("modal", wide && "wide")} role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-header">
          <div>
            <h2>{title}</h2>
            {description ? <p className="muted small" style={{ marginTop: 3 }}>{description}</p> : null}
          </div>
          <IconButton icon="close" label="Close" onClick={onClose} />
        </div>
        <div className="modal-body">{children}</div>
        {footer ? <div className="modal-footer">{footer}</div> : null}
      </div>
    </div>
  );
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel = "Delete",
  danger = true,
  busy,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: string;
  description: string;
  confirmLabel?: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <Modal
      open={open}
      title={title}
      description={description}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant={danger ? "danger" : "primary"}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? <Spinner /> : null}
            {confirmLabel}
          </Button>
        </>
      }
    >
      <Alert tone={danger ? "error" : "warning"}>
        This action cannot be undone from the web client.
      </Alert>
    </Modal>
  );
}

export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  return (
    <Button
      size="sm"
      variant="secondary"
      onClick={() => {
        void navigator.clipboard?.writeText(value);
      }}
    >
      {label}
    </Button>
  );
}

export function SectionTitle({
  title,
  action,
}: {
  title: string;
  action?: ReactNode;
}) {
  return (
    <div className="section-title">
      <h2>{title}</h2>
      {action}
    </div>
  );
}
