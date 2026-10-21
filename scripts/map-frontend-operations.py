#!/usr/bin/env python3
"""Map every frontend GraphQL document to the live Squid GraphQL schema.

Reads the generated Apollo documents in the upstream checkout, validates each
operation's selection set (fields and arguments) against a running Squid
server, and writes a machine-readable JSON map plus a Markdown report.

Usage:
  python3 scripts/map-frontend-operations.py \
      --squid-url http://127.0.0.1:3111 \
      --out-json contracts/frontend-operations.json \
      --out-md docs/frontend-operation-map.md
"""

import argparse
import json
import os
import re
import sys
import urllib.request

DOC_RE = re.compile(r"^export const ([A-Za-z0-9_]+)Document = (\{.*\}) as unknown as DocumentNode")
ROOT_TYPES = ("Query", "Mutation", "Subscription")
COMPOSITE = ("OBJECT", "INTERFACE", "UNION")


def load_documents(path):
    documents = {}
    with open(path) as fh:
        for line in fh:
            match = DOC_RE.match(line.strip())
            if not match:
                continue
            name, raw = match.groups()
            try:
                documents[name] = json.loads(raw)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"cannot parse {name} in {path}: {exc}")
    return documents


def introspect(url):
    query = """
    query {
      __schema {
        queryType { name }
        mutationType { name }
        subscriptionType { name }
        types {
          kind name
          fields { name args { name } }
          inputFields { name }
          interfaces { kind name }
          possibleTypes { kind name }
        }
      }
    }
    """
    request = urllib.request.Request(
        url,
        data=json.dumps({"query": query}).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = json.loads(response.read())
    if "errors" in payload:
        raise SystemExit(f"introspection failed at {url}: {payload['errors']}")
    return payload["data"]["__schema"]


def unwrap(type_ref):
    while type_ref and type_ref["kind"] in ("NON_NULL", "LIST"):
        type_ref = type_ref.get("ofType")
    return type_ref or {}


def ast_type_name(type_ref):
    while type_ref and type_ref.get("kind") in ("NON_NULL", "LIST"):
        type_ref = type_ref.get("type")
    name = (type_ref or {}).get("name")
    if isinstance(name, dict):
        return name.get("value")
    return name


class Validator:
    def __init__(self, schema):
        self.schema = schema
        self.types = {item["name"]: item for item in schema["types"]}
        self.roots = {
            "query": (schema.get("queryType") or {}).get("name"),
            "mutation": (schema.get("mutationType") or {}).get("name"),
            "subscription": (schema.get("subscriptionType") or {}).get("name"),
        }
        self.fragments = {}

    def add_fragments(self, document):
        for definition in document.get("definitions", []):
            if definition.get("kind") == "FragmentDefinition":
                self.fragments[definition["name"]["value"]] = definition

    def field_map(self, type_name):
        type_def = self.types.get(type_name)
        if not type_def:
            return {}
        return {field["name"]: field for field in (type_def.get("fields") or [])}

    def validate(self, selection_set, type_name, path, issues, seen_fragments=None):
        seen_fragments = seen_fragments or set()
        type_def = self.types.get(type_name) or {}
        if type_def.get("kind") == "UNION":
            fields = {}
        else:
            fields = self.field_map(type_name)

        for selection in selection_set.get("selections", []):
            kind = selection.get("kind")
            if kind == "Field":
                name = selection["name"]["value"]
                if name == "__typename":
                    continue
                field_path = f"{path}.{name}" if path else name
                field = fields.get(name)
                if field is None:
                    issues.append({"path": field_path, "issue": "missing field"})
                    continue
                provided = {
                    arg["name"]["value"] for arg in (selection.get("arguments") or [])
                }
                known = {arg["name"] for arg in (field.get("args") or [])}
                for arg in sorted(provided - known):
                    issues.append({"path": field_path, "issue": f"missing argument '{arg}'"})
                target = unwrap(field.get("type"))
                if selection.get("selectionSet") and target.get("kind") in COMPOSITE:
                    self.validate(
                        selection["selectionSet"],
                        target.get("name"),
                        field_path,
                        issues,
                        seen_fragments,
                    )
            elif kind == "InlineFragment":
                condition = (selection.get("typeCondition") or {}).get("name")
                target = condition or type_name
                if "selectionSet" in selection:
                    self.validate(
                        selection["selectionSet"], target, path, issues, seen_fragments
                    )
            elif kind == "FragmentSpread":
                name = selection["name"]["value"]
                fragment = self.fragments.get(name)
                if fragment is None:
                    issues.append({"path": path, "issue": f"unknown fragment '{name}'"})
                    continue
                if name in seen_fragments:
                    continue
                condition = fragment["typeCondition"]["name"]
                self.validate(
                    fragment["selectionSet"],
                    condition,
                    path,
                    issues,
                    seen_fragments | {name},
                )

    def validate_operation(self, document, operation):
        kind = operation["operation"]
        root_name = self.roots.get(kind)
        if not root_name:
            return "MISSING", [{"path": root_name or kind, "issue": "no root type"}]
        issues = []
        self.validate(operation["selectionSet"], root_name, "", issues)
        if any("missing field" in issue["issue"] and issue["path"] == root for issue in issues for root in []):
            pass
        root_fields = {
            field["name"]["value"]
            for field in operation["selectionSet"].get("selections", [])
            if field.get("kind") == "Field"
        }
        root_fields.discard("__typename")
        roots = self.field_map(root_name)
        if root_fields and not (root_fields & set(roots)):
            return "MISSING", issues or [{"path": kind, "issue": "no root field match"}]
        if any(issue["issue"] == "missing field" for issue in issues):
            return "PARTIAL", issues
        if issues:
            return "PARTIAL", issues
        return "FULL", []


def operation_rows(documents, validator):
    rows = []
    for name, document in sorted(documents.items()):
        for definition in document.get("definitions", []):
            if definition.get("kind") != "OperationDefinition":
                continue
            validator.add_fragments(document)
            status, issues = validator.validate_operation(document, definition)
            variables = [
                {
                    "name": var["variable"]["name"]["value"],
                    "type": ast_type_name(var["type"]),
                }
                for var in (definition.get("variableDefinitions") or [])
            ]
            roots = [
                selection["name"]["value"]
                for selection in definition["selectionSet"].get("selections", [])
                if selection.get("kind") == "Field" and selection["name"]["value"] != "__typename"
            ]
            rows.append(
                {
                    "document": name,
                    "operation": definition.get("name", {}).get("value") or name,
                    "kind": definition["operation"],
                    "rootFields": roots,
                    "variables": variables,
                    "status": status,
                    "issues": issues,
                }
            )
    return rows


def write_markdown(rows, out_path, summary):
    lines = [
        "# Frontend GraphQL operation map",
        "",
        "Generated by `scripts/map-frontend-operations.py` against a running Squid",
        "server. Each row is one operation exported by the frontend Apollo",
        "documents. Status is schema-level: **FULL** means the root field, every",
        "selected field/argument and every fragment resolve in the Squid schema;",
        "**PARTIAL** means the root exists but some selections do not; **MISSING**",
        "means the root field is absent. Runtime correctness and input-object field",
        "coverage are tracked separately (see `docs/api-compatibility.md`).",
        "",
        f"- Data schema (`generated/graphql.ts`): {summary['data']}",
        f"- Metadata schema (`generated-metadata/graphql.ts`): {summary['metadata']}",
        "",
    ]
    for section, section_rows in rows.items():
        counts = {"FULL": 0, "PARTIAL": 0, "MISSING": 0}
        for row in section_rows:
            counts[row["status"]] += 1
        lines.append(f"## {section}")
        lines.append("")
        lines.append(
            f"Operations: {len(section_rows)} — FULL {counts['FULL']}, "
            f"PARTIAL {counts['PARTIAL']}, MISSING {counts['MISSING']}."
        )
        lines.append("")
        lines.append("| Operation | Kind | Root field(s) | Status | Missing selections |")
        lines.append("| --- | --- | --- | --- | --- |")
        for row in section_rows:
            issues = sorted({issue["path"] for issue in row["issues"]})
            missing = ", ".join(f"`{item}`" for item in issues[:8])
            if len(issues) > 8:
                missing += f", +{len(issues) - 8} more"
            roots = ", ".join(f"`{root}`" for root in row["rootFields"])
            lines.append(
                f"| `{row['operation']}` | {row['kind']} | {roots} | "
                f"**{row['status']}** | {missing or '—'} |"
            )
        lines.append("")
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--squid-url", default="http://127.0.0.1:3111")
    parser.add_argument("--reference", default="reference/twenty")
    parser.add_argument("--out-json", default="contracts/frontend-operations.json")
    parser.add_argument("--out-md", default="docs/frontend-operation-map.md")
    args = parser.parse_args()

    data_docs = load_documents(
        os.path.join(args.reference, "packages/twenty-front/src/generated/graphql.ts")
    )
    metadata_docs = load_documents(
        os.path.join(
            args.reference,
            "packages/twenty-front/src/generated-metadata/graphql.ts",
        )
    )

    report = {"generatedBy": "scripts/map-frontend-operations.py", "schemas": {}}
    markdown_rows = {}
    summary = {"data": "", "metadata": ""}
    for section, docs, endpoint in (
        ("data", data_docs, "/graphql"),
        ("metadata", metadata_docs, "/metadata"),
    ):
        schema = introspect(args.squid_url.rstrip("/") + endpoint)
        validator = Validator(schema)
        for document in docs.values():
            validator.add_fragments(document)
        rows = operation_rows(docs, validator)
        missing_roots = sorted(
            {root for row in rows if row["status"] == "MISSING" for root in row["rootFields"]}
        )
        report["schemas"][section] = {
            "endpoint": endpoint,
            "documents": len(docs),
            "missingRootFields": missing_roots,
            "operations": rows,
        }
        markdown_rows[section] = rows
        counts = {"FULL": 0, "PARTIAL": 0, "MISSING": 0}
        for row in rows:
            counts[row["status"]] += 1
        summary[section] = (
            f"{len(rows)} operations (FULL {counts['FULL']}, "
            f"PARTIAL {counts['PARTIAL']}, MISSING {counts['MISSING']})"
        )

    report["summary"] = summary
    os.makedirs(os.path.dirname(args.out_json) or ".", exist_ok=True)
    with open(args.out_json, "w") as fh:
        json.dump(report, fh, indent=2)
    write_markdown(markdown_rows, args.out_md, summary)
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    sys.exit(main())
