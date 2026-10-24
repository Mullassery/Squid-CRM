"use client";

import { useState } from "react";
import Link from "next/link";
import { useSession } from "@/hooks/useSession";
import { Alert, Button, Field, Spinner } from "@/components/ui";

export default function SignUpPage() {
  const { signUp } = useSession();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [workspaceName, setWorkspaceName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await signUp(email, password, workspaceName);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Sign up failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-card">
      <div className="auth-brand">
        <span className="brand-mark">S</span>
        <div>
          <div className="brand-name">Squid CRM</div>
          <div className="brand-workspace">Self-hosted workspace</div>
        </div>
      </div>
      <h1>Create your workspace</h1>
      <p className="auth-sub">
        The first user becomes the workspace owner. You can invite more members
        later.
      </p>
      {error ? <Alert tone="error">{error}</Alert> : null}
      <form className="auth-form" onSubmit={onSubmit}>
        <Field label="Workspace name" required>
          <input
            className="input"
            value={workspaceName}
            onChange={(event) => setWorkspaceName(event.target.value)}
            placeholder="Acme Sales"
            required
          />
        </Field>
        <Field label="Email" required>
          <input
            className="input"
            type="email"
            autoComplete="email"
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            placeholder="you@company.com"
            required
          />
        </Field>
        <Field label="Password" required help="At least 8 characters.">
          <input
            className="input"
            type="password"
            autoComplete="new-password"
            minLength={8}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            required
          />
        </Field>
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? <Spinner /> : null}
          Create workspace
        </Button>
      </form>
      <div className="auth-footer">
        Already have an account? <Link href="/signin">Sign in</Link>
      </div>
    </div>
  );
}
