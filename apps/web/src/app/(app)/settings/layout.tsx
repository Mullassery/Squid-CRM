"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const items = [
  { href: "/settings", label: "General" },
  { href: "/settings/members", label: "Members" },
  { href: "/settings/roles", label: "Roles & permissions" },
  { href: "/settings/api-keys", label: "API keys" },
  { href: "/settings/webhooks", label: "Webhooks" },
  { href: "/settings/metadata", label: "Data model" },
  { href: "/settings/views", label: "Views" },
  { href: "/settings/data", label: "Custom objects" },
  { href: "/settings/jobs", label: "Jobs" },
  { href: "/settings/console", label: "API console" },
  { href: "/settings/profile", label: "Profile" },
];

export default function SettingsLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  return (
    <div className="settings-grid">
      <nav className="settings-nav">
        {items.map((item) => {
          const active =
            item.href === "/settings"
              ? pathname === "/settings"
              : pathname.startsWith(item.href);
          return (
            <Link key={item.href} href={item.href} className={active ? "active" : undefined}>
              {item.label}
            </Link>
          );
        })}
      </nav>
      <div style={{ minWidth: 0 }}>{children}</div>
    </div>
  );
}
