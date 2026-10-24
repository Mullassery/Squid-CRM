"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { authApi, formatApiError } from "@/lib/api";
import type { Session } from "@/lib/types";

interface SessionContextValue {
  session: Session | null;
  loading: boolean;
  error: string | null;
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (email: string, password: string, workspaceName: string) => Promise<void>;
  signOut: () => Promise<void>;
  refresh: () => Promise<void>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const [session, setSession] = useState<Session | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const next = await authApi.me();
      setSession(next);
      setError(null);
    } catch {
      setSession(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const signIn = useCallback(
    async (email: string, password: string) => {
      setError(null);
      try {
        await authApi.signin(email, password);
        const next = await authApi.me();
        setSession(next);
        router.replace("/dashboard");
      } catch (cause) {
        const message = formatApiError(cause);
        setError(message);
        throw new Error(message);
      }
    },
    [router],
  );

  const signUp = useCallback(
    async (email: string, password: string, workspaceName: string) => {
      setError(null);
      try {
        await authApi.signup(email, password, workspaceName);
        const next = await authApi.me();
        setSession(next);
        router.replace("/dashboard");
      } catch (cause) {
        const message = formatApiError(cause);
        setError(message);
        throw new Error(message);
      }
    },
    [router],
  );

  const signOut = useCallback(async () => {
    try {
      await authApi.signout();
    } finally {
      setSession(null);
      router.replace("/signin");
    }
  }, [router]);

  const value = useMemo(
    () => ({ session, loading, error, signIn, signUp, signOut, refresh }),
    [session, loading, error, signIn, signUp, signOut, refresh],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): SessionContextValue {
  const context = useContext(SessionContext);
  if (!context) {
    throw new Error("useSession must be used within SessionProvider");
  }
  return context;
}
