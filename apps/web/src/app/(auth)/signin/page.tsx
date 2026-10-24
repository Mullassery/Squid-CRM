"use client";

import { useState } from "react";
import Link from "next/link";
import { useSession } from "@/hooks/useSession";
import { Alert, Button, Field, Spinner } from "@/components/ui";

export default function SignInPage() {
  const { signIn } = useSession();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await signIn(email, password);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Sign in failed.");
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
      <h1>Welcome back</h1>
      <p className="auth-sub">Sign in to your workspace.</p>
      {error ? <Alert tone="error">{error}</Alert> : null}
      <form className="auth-form" onSubmit={onSubmit}>
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
        <Field label="Password" required>
          <input
            className="input"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            required
          />
        </Field>
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? <Spinner /> : null}
          Sign in
        </Button>
      </form>
      <div className="auth-footer">
        New here? <Link href="/signup">Create a workspace</Link>
      </div>
    </div>
  );
}
