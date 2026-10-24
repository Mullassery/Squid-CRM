import { AppGate, AppShell } from "@/components/app-shell";
import { LookupProvider } from "@/hooks/useLookups";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  return (
    <AppGate>
      <LookupProvider>
        <AppShell>{children}</AppShell>
      </LookupProvider>
    </AppGate>
  );
}
