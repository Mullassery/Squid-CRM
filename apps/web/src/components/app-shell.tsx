"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { Icon, type IconName } from "@/components/icons";
import { Avatar, Button, Spinner } from "@/components/ui";
import { useSession } from "@/hooks/useSession";
import { useAsyncData } from "@/hooks/useAsyncData";
import { gql, queries } from "@/lib/graphql";
import { resources } from "@/lib/resources";
import type { ObjectMetadata } from "@/lib/types";

const resourceIcons: Record<string, IconName> = {
  people: "users",
  companies: "building",
  opportunities: "target",
  tasks: "check",
  notes: "note",
  taskTargets: "link",
  noteTargets: "link",
};

function NavLink({
  href,
  icon,
  label,
  active,
}: {
  href: string;
  icon: IconName;
  label: string;
  active: boolean;
}) {
  return (
    <Link className={`nav-link${active ? " active" : ""}`} href={href}>
      <span className="nav-icon">
        <Icon name={icon} size={17} />
      </span>
      {label}
    </Link>
  );
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const { session, signOut } = useSession();
  const [query, setQuery] = useState("");
  const [signingOut, setSigningOut] = useState(false);

  const { data: objects } = useAsyncData(
    () =>
      gql<{ objects: { edges: { node: ObjectMetadata }[] } }>(queries.objects, {
        first: 100,
      }),
    [],
  );
  const customObjects = (objects?.objects.edges ?? [])
    .map((edge) => edge.node)
    .filter((object) => !object.isSystem);

  if (!session) {
    return (
      <div className="loading-block" style={{ minHeight: "100vh", justifyContent: "center" }}>
        <Spinner large />
      </div>
    );
  }

  const displayName =
    [session.user.email.split("@")[0], session.role.toLowerCase()].join(" · ") ||
    session.user.email;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <Link className="brand" href="/dashboard">
          <span className="brand-mark">S</span>
          <div>
            <div className="brand-name">Squid CRM</div>
            <div className="brand-workspace">{session.workspace.displayName}</div>
          </div>
        </Link>

        <nav className="nav-section">
          <NavLink
            href="/dashboard"
            icon="dashboard"
            label="Dashboard"
            active={pathname === "/dashboard"}
          />
          <NavLink
            href="/search"
            icon="search"
            label="Search"
            active={pathname === "/search"}
          />
        </nav>

        <nav className="nav-section">
          <div className="nav-label">Records</div>
          {resources.map((resource) => (
            <NavLink
              key={resource.key}
              href={`/objects/${resource.key}`}
              icon={resourceIcons[resource.key] ?? "database"}
              label={resource.labelPlural}
              active={pathname.startsWith(`/objects/${resource.key}`)}
            />
          ))}
        </nav>

        {customObjects.length > 0 ? (
          <nav className="nav-section">
            <div className="nav-label">Custom objects</div>
            {customObjects.map((object) => (
              <NavLink
                key={object.id}
                href={`/custom/${object.nameSingular}`}
                icon="database"
                label={object.labelPlural}
                active={pathname.startsWith(`/custom/${object.nameSingular}`)}
              />
            ))}
          </nav>
        ) : null}

        <nav className="nav-section">
          <div className="nav-label">Workspace</div>
          <NavLink
            href="/settings"
            icon="settings"
            label="Settings"
            active={pathname.startsWith("/settings")}
          />
          <NavLink
            href="/files"
            icon="file"
            label="Files"
            active={pathname === "/files"}
          />
        </nav>

        <div className="sidebar-footer">
          <Link className="user-chip" href="/settings/profile">
            <Avatar name={session.user.email} size="sm" />
            <span className="user-meta">
              <span className="user-name">{session.user.email.split("@")[0]}</span>
              <span className="user-email">{displayName}</span>
            </span>
          </Link>
          <Button
            variant="ghost"
            size="sm"
            style={{ width: "100%", justifyContent: "flex-start", color: "#94a3b8" }}
            disabled={signingOut}
            onClick={() => {
              setSigningOut(true);
              void signOut().finally(() => setSigningOut(false));
            }}
          >
            <Icon name="logout" size={16} />
            Sign out
          </Button>
        </div>
      </aside>

      <div className="main">
        <header className="topbar">
          <form
            className="searchbox"
            onSubmit={(event) => {
              event.preventDefault();
              if (query.trim()) {
                router.push(`/search?q=${encodeURIComponent(query.trim())}`);
              }
            }}
          >
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search people, companies, opportunities…"
              aria-label="Global search"
            />
          </form>
          <div className="header-actions">
            <span className="badge accent" title={`Role: ${session.role}`}>
              {session.role.toLowerCase()}
            </span>
            <Link className="btn btn-primary btn-sm" href="/objects/people/new">
              <Icon name="plus" size={15} />
              New person
            </Link>
          </div>
        </header>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}

export function AppGate({ children }: { children: React.ReactNode }) {
  const { session, loading } = useSession();
  const router = useRouter();

  useEffect(() => {
    if (!loading && !session) {
      router.replace("/signin");
    }
  }, [loading, session, router]);

  if (loading) {
    return (
      <div className="loading-block" style={{ minHeight: "100vh", justifyContent: "center" }}>
        <Spinner large />
      </div>
    );
  }

  if (!session) return null;

  return <>{children}</>;
}