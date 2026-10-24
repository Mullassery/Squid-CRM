import type { Metadata } from "next";
import "./globals.css";
import { SessionProvider } from "@/hooks/useSession";

export const metadata: Metadata = {
  title: {
    default: "Squid CRM",
    template: "%s · Squid CRM",
  },
  description: "Self-hosted CRM with a Rust API and a typed Next.js client.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>
        <SessionProvider>{children}</SessionProvider>
      </body>
    </html>
  );
}
