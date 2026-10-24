import type { JsonObject, JsonValue } from "./types";
import {
  currencyInputValue,
  fromDateInput,
  fromDateTimeInput,
  splitList,
  toDateInput,
  toDateTimeInput,
} from "./format";

export type FieldKind =
  | "text"
  | "textarea"
  | "email"
  | "number"
  | "money"
  | "date"
  | "datetime"
  | "select"
  | "boolean"
  | "json"
  | "relation";

export type RelationKind =
  | "person"
  | "company"
  | "opportunity"
  | "task"
  | "note"
  | "member";

export interface FieldDef {
  key: string;
  label: string;
  kind: FieldKind;
  required?: boolean;
  options?: { value: string; label: string }[];
  relation?: RelationKind;
  placeholder?: string;
  help?: string;
  list?: boolean;
  detail?: boolean;
  readOnly?: boolean;
  wide?: boolean;
}

export type FormValues = Record<string, string>;
export type GenericRecord = Record<string, unknown>;

export interface ResourceConfig {
  key: string;
  objectName: string;
  label: string;
  labelPlural: string;
  description: string;
  icon: string;
  accent: string;
  restPath: string;
  listKey: string;
  createKey: string;
  updateKey: string;
  deleteKey: string;
  defaultSort: string;
  fields: FieldDef[];
  toForm: (record: GenericRecord) => FormValues;
  buildPayload: (values: FormValues) => JsonObject;
  title: (record: GenericRecord) => string;
  subtitle?: (record: GenericRecord) => string;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function uuidOrNull(value: string | undefined): JsonValue {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : null;
}

function optionalText(value: string | undefined): JsonValue {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : null;
}

function numberOrNull(value: string | undefined): JsonValue {
  const trimmed = value?.trim() ?? "";
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

function moneyPayload(value: string | undefined, currencyCode: string | undefined): JsonObject {
  const amount = numberOrNull(value);
  return {
    amountMicros: amount === null ? null : Math.round(Number(amount) * 1_000_000),
    currencyCode: currencyCode?.trim() || "USD",
  };
}

const statusOptions = [
  { value: "TODO", label: "To do" },
  { value: "IN_PROGRESS", label: "In progress" },
  { value: "DONE", label: "Done" },
];

const stageOptions = [
  { value: "NEW", label: "New" },
  { value: "SCREENING", label: "Screening" },
  { value: "MEETING", label: "Meeting" },
  { value: "PROPOSAL", label: "Proposal" },
  { value: "CUSTOMER", label: "Customer" },
];

const people: ResourceConfig = {
  key: "people",
  objectName: "person",
  label: "Person",
  labelPlural: "People",
  description: "Contacts, emails and the companies they work with.",
  icon: "users",
  accent: "#2563eb",
  restPath: "/rest/people",
  listKey: "people",
  createKey: "createPerson",
  updateKey: "updatePerson",
  deleteKey: "deletePerson",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "firstName", label: "First name", kind: "text", required: true, list: true },
    { key: "lastName", label: "Last name", kind: "text" },
    { key: "primaryEmail", label: "Email", kind: "email", list: true },
    { key: "jobTitle", label: "Job title", kind: "text", list: true },
    { key: "companyId", label: "Company", kind: "relation", relation: "company", list: true },
    { key: "phone", label: "Phone", kind: "text" },
    { key: "additionalEmails", label: "Additional emails", kind: "text", help: "Comma separated" },
    { key: "linkedinUrl", label: "LinkedIn URL", kind: "text" },
    { key: "avatarUrl", label: "Avatar URL", kind: "text" },
  ],
  toForm(record) {
    const name = (record.name ?? {}) as Record<string, unknown>;
    const emails = (record.emails ?? {}) as Record<string, unknown>;
    const phones = (record.phones ?? {}) as Record<string, unknown>;
    const linkedin = (record.linkedinLink ?? {}) as Record<string, unknown>;
    const additional = Array.isArray(emails.additionalEmails)
      ? (emails.additionalEmails as string[]).join(", ")
      : "";
    return {
      firstName: text(name.firstName),
      lastName: text(name.lastName),
      primaryEmail: text(emails.primaryEmail),
      additionalEmails: additional,
      jobTitle: text(record.jobTitle),
      companyId: text(record.companyId),
      phone: text(phones.primaryPhoneNumber),
      linkedinUrl: text(linkedin.primaryLinkUrl),
      avatarUrl: text(record.avatarUrl),
    };
  },
  buildPayload(values) {
    return {
      name: { firstName: values.firstName?.trim() ?? "", lastName: values.lastName?.trim() ?? "" },
      emails: {
        primaryEmail: values.primaryEmail?.trim() ?? "",
        additionalEmails: splitList(values.additionalEmails),
      },
      jobTitle: optionalText(values.jobTitle),
      companyId: uuidOrNull(values.companyId),
      phones: {
        primaryPhoneNumber: values.phone?.trim() ?? "",
        primaryPhoneCountryCode: "",
        primaryPhoneCallingCode: "",
        additionalPhones: [],
      },
      linkedinLink: {
        primaryLinkLabel: "",
        primaryLinkUrl: values.linkedinUrl?.trim() ?? "",
        secondaryLinks: [],
      },
      avatarUrl: values.avatarUrl?.trim() ?? "",
    };
  },
  title(record) {
    const name = (record.name ?? {}) as Record<string, unknown>;
    return `${text(name.firstName)} ${text(name.lastName)}`.trim() || "Untitled person";
  },
  subtitle(record) {
    const emails = (record.emails ?? {}) as Record<string, unknown>;
    return text(emails.primaryEmail);
  },
};

const companies: ResourceConfig = {
  key: "companies",
  objectName: "company",
  label: "Company",
  labelPlural: "Companies",
  description: "Accounts, domains and revenue.",
  icon: "building",
  accent: "#0f766e",
  restPath: "/rest/companies",
  listKey: "companies",
  createKey: "createCompany",
  updateKey: "updateCompany",
  deleteKey: "deleteCompany",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "name", label: "Name", kind: "text", required: true, list: true },
    { key: "domain", label: "Domain", kind: "text", list: true },
    { key: "annualRevenue", label: "Annual revenue", kind: "money", list: true },
    { key: "currencyCode", label: "Currency", kind: "text" },
    { key: "accountOwnerId", label: "Account owner", kind: "relation", relation: "member", list: true },
    { key: "linkedinUrl", label: "LinkedIn URL", kind: "text" },
    { key: "addressStreet1", label: "Street", kind: "text", wide: true },
    { key: "addressCity", label: "City", kind: "text" },
    { key: "addressPostcode", label: "Postcode", kind: "text" },
    { key: "addressState", label: "State", kind: "text" },
    { key: "addressCountry", label: "Country", kind: "text" },
  ],
  toForm(record) {
    const domain = (record.domainName ?? {}) as Record<string, unknown>;
    const linkedin = (record.linkedinLink ?? {}) as Record<string, unknown>;
    const address = (record.address ?? {}) as Record<string, unknown>;
    return {
      name: text(record.name),
      domain: text(domain.primaryLinkUrl),
      annualRevenue: currencyInputValue(record.annualRevenue as never),
      currencyCode: text((record.annualRevenue as Record<string, unknown> | undefined)?.currencyCode) || "USD",
      accountOwnerId: text(record.accountOwnerId),
      linkedinUrl: text(linkedin.primaryLinkUrl),
      addressStreet1: text(address.addressStreet1),
      addressCity: text(address.addressCity),
      addressPostcode: text(address.addressPostcode),
      addressState: text(address.addressState),
      addressCountry: text(address.addressCountry),
    };
  },
  buildPayload(values) {
    return {
      name: optionalText(values.name),
      domainName: {
        primaryLinkLabel: "",
        primaryLinkUrl: values.domain?.trim() ?? "",
        secondaryLinks: [],
      },
      annualRevenue: moneyPayload(values.annualRevenue, values.currencyCode),
      accountOwnerId: uuidOrNull(values.accountOwnerId),
      linkedinLink: {
        primaryLinkLabel: "",
        primaryLinkUrl: values.linkedinUrl?.trim() ?? "",
        secondaryLinks: [],
      },
      address: {
        addressStreet1: optionalText(values.addressStreet1),
        addressCity: optionalText(values.addressCity),
        addressPostcode: optionalText(values.addressPostcode),
        addressState: optionalText(values.addressState),
        addressCountry: optionalText(values.addressCountry),
      },
    };
  },
  title(record) {
    return text(record.name) || "Untitled company";
  },
  subtitle(record) {
    const domain = (record.domainName ?? {}) as Record<string, unknown>;
    return text(domain.primaryLinkUrl);
  },
};

const opportunities: ResourceConfig = {
  key: "opportunities",
  objectName: "opportunity",
  label: "Opportunity",
  labelPlural: "Opportunities",
  description: "Pipeline, amounts and close dates.",
  icon: "target",
  accent: "#b45309",
  restPath: "/rest/opportunities",
  listKey: "opportunities",
  createKey: "createOpportunity",
  updateKey: "updateOpportunity",
  deleteKey: "deleteOpportunity",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "name", label: "Name", kind: "text", required: true, list: true },
    { key: "amount", label: "Amount", kind: "money", list: true },
    { key: "currencyCode", label: "Currency", kind: "text" },
    { key: "stage", label: "Stage", kind: "select", options: stageOptions, list: true },
    { key: "closeDate", label: "Close date", kind: "date", list: true },
    { key: "companyId", label: "Company", kind: "relation", relation: "company", list: true },
    { key: "pointOfContactId", label: "Point of contact", kind: "relation", relation: "person" },
    { key: "ownerId", label: "Owner", kind: "relation", relation: "member" },
  ],
  toForm(record) {
    return {
      name: text(record.name),
      amount: currencyInputValue(record.amount as never),
      currencyCode: text((record.amount as Record<string, unknown> | undefined)?.currencyCode) || "USD",
      stage: text(record.stage) || "NEW",
      closeDate: toDateInput(text(record.closeDate)),
      companyId: text(record.companyId),
      pointOfContactId: text(record.pointOfContactId),
      ownerId: text(record.ownerId),
    };
  },
  buildPayload(values) {
    return {
      name: optionalText(values.name),
      amount: moneyPayload(values.amount, values.currencyCode),
      stage: values.stage || "NEW",
      closeDate: fromDateInput(values.closeDate ?? "") ?? null,
      companyId: uuidOrNull(values.companyId),
      pointOfContactId: uuidOrNull(values.pointOfContactId),
      ownerId: uuidOrNull(values.ownerId),
    };
  },
  title(record) {
    return text(record.name) || "Untitled opportunity";
  },
  subtitle(record) {
    return text(record.stage);
  },
};

const tasks: ResourceConfig = {
  key: "tasks",
  objectName: "task",
  label: "Task",
  labelPlural: "Tasks",
  description: "Work to do, with status and due dates.",
  icon: "check",
  accent: "#7c3aed",
  restPath: "/rest/tasks",
  listKey: "tasks",
  createKey: "createTask",
  updateKey: "updateTask",
  deleteKey: "deleteTask",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "title", label: "Title", kind: "text", required: true, list: true },
    { key: "status", label: "Status", kind: "select", options: statusOptions, list: true },
    { key: "dueAt", label: "Due", kind: "datetime", list: true },
    { key: "assigneeId", label: "Assignee", kind: "relation", relation: "member", list: true },
    { key: "bodyMarkdown", label: "Notes", kind: "textarea", wide: true },
  ],
  toForm(record) {
    const body = (record.bodyV2 ?? {}) as Record<string, unknown>;
    return {
      title: text(record.title),
      status: text(record.status) || "TODO",
      dueAt: toDateTimeInput(text(record.dueAt)),
      assigneeId: text(record.assigneeId),
      bodyMarkdown: text(body.markdown),
    };
  },
  buildPayload(values) {
    return {
      title: optionalText(values.title),
      status: values.status || "TODO",
      dueAt: fromDateTimeInput(values.dueAt ?? "") ?? null,
      assigneeId: uuidOrNull(values.assigneeId),
      bodyV2: { markdown: values.bodyMarkdown?.trim() || null, blocknote: null },
    };
  },
  title(record) {
    return text(record.title) || "Untitled task";
  },
  subtitle(record) {
    return text(record.status);
  },
};

const notes: ResourceConfig = {
  key: "notes",
  objectName: "note",
  label: "Note",
  labelPlural: "Notes",
  description: "Long-form notes and context.",
  icon: "note",
  accent: "#be123c",
  restPath: "/rest/notes",
  listKey: "notes",
  createKey: "createNote",
  updateKey: "updateNote",
  deleteKey: "deleteNote",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "title", label: "Title", kind: "text", required: true, list: true },
    { key: "bodyMarkdown", label: "Body", kind: "textarea", list: true, wide: true },
  ],
  toForm(record) {
    const body = (record.bodyV2 ?? {}) as Record<string, unknown>;
    return {
      title: text(record.title),
      bodyMarkdown: text(body.markdown),
    };
  },
  buildPayload(values) {
    return {
      title: optionalText(values.title),
      bodyV2: { markdown: values.bodyMarkdown?.trim() || null, blocknote: null },
    };
  },
  title(record) {
    return text(record.title) || "Untitled note";
  },
  subtitle(record) {
    const body = (record.bodyV2 ?? {}) as Record<string, unknown>;
    return text(body.markdown).slice(0, 120);
  },
};

const taskTargets: ResourceConfig = {
  key: "taskTargets",
  objectName: "taskTarget",
  label: "Task target",
  labelPlural: "Task targets",
  description: "Links tasks to companies, people and opportunities.",
  icon: "link",
  accent: "#0369a1",
  restPath: "/rest/taskTargets",
  listKey: "taskTargets",
  createKey: "createTaskTarget",
  updateKey: "updateTaskTarget",
  deleteKey: "deleteTaskTarget",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "taskId", label: "Task", kind: "relation", relation: "task", required: true, list: true },
    { key: "targetCompanyId", label: "Company", kind: "relation", relation: "company", list: true },
    { key: "targetPersonId", label: "Person", kind: "relation", relation: "person", list: true },
    {
      key: "targetOpportunityId",
      label: "Opportunity",
      kind: "relation",
      relation: "opportunity",
      list: true,
    },
  ],
  toForm(record) {
    return {
      taskId: text(record.taskId),
      targetCompanyId: text(record.targetCompanyId),
      targetPersonId: text(record.targetPersonId),
      targetOpportunityId: text(record.targetOpportunityId),
    };
  },
  buildPayload(values) {
    return {
      taskId: uuidOrNull(values.taskId),
      targetCompanyId: uuidOrNull(values.targetCompanyId),
      targetPersonId: uuidOrNull(values.targetPersonId),
      targetOpportunityId: uuidOrNull(values.targetOpportunityId),
    };
  },
  title() {
    return "Task target";
  },
};

const noteTargets: ResourceConfig = {
  key: "noteTargets",
  objectName: "noteTarget",
  label: "Note target",
  labelPlural: "Note targets",
  description: "Links notes to companies, people and opportunities.",
  icon: "link",
  accent: "#4d7c0f",
  restPath: "/rest/noteTargets",
  listKey: "noteTargets",
  createKey: "createNoteTarget",
  updateKey: "updateNoteTarget",
  deleteKey: "deleteNoteTarget",
  defaultSort: "createdAt[DescNullsLast]",
  fields: [
    { key: "noteId", label: "Note", kind: "relation", relation: "note", required: true, list: true },
    { key: "targetCompanyId", label: "Company", kind: "relation", relation: "company", list: true },
    { key: "targetPersonId", label: "Person", kind: "relation", relation: "person", list: true },
    {
      key: "targetOpportunityId",
      label: "Opportunity",
      kind: "relation",
      relation: "opportunity",
      list: true,
    },
  ],
  toForm(record) {
    return {
      noteId: text(record.noteId),
      targetCompanyId: text(record.targetCompanyId),
      targetPersonId: text(record.targetPersonId),
      targetOpportunityId: text(record.targetOpportunityId),
    };
  },
  buildPayload(values) {
    return {
      noteId: uuidOrNull(values.noteId),
      targetCompanyId: uuidOrNull(values.targetCompanyId),
      targetPersonId: uuidOrNull(values.targetPersonId),
      targetOpportunityId: uuidOrNull(values.targetOpportunityId),
    };
  },
  title() {
    return "Note target";
  },
};

export const resources: ResourceConfig[] = [
  people,
  companies,
  opportunities,
  tasks,
  notes,
  taskTargets,
  noteTargets,
];

export const resourcesByKey: Record<string, ResourceConfig> = Object.fromEntries(
  resources.map((resource) => [resource.key, resource]),
);

export function isResourceKey(value: string): value is string {
  return Boolean(resourcesByKey[value]);
}

export function emptyForm(resource: ResourceConfig): FormValues {
  const values: FormValues = {};
  for (const field of resource.fields) {
    values[field.key] = field.kind === "select" ? field.options?.[0]?.value ?? "" : "";
  }
  return values;
}

export const relationConfig: Record<
  RelationKind,
  { label: string; path: string; listKey: string; title: (record: GenericRecord) => string }
> = {
  person: { label: "Person", path: "/rest/people", listKey: "people", title: (record) => people.title(record) },
  company: {
    label: "Company",
    path: "/rest/companies",
    listKey: "companies",
    title: (record) => companies.title(record),
  },
  opportunity: {
    label: "Opportunity",
    path: "/rest/opportunities",
    listKey: "opportunities",
    title: (record) => opportunities.title(record),
  },
  task: { label: "Task", path: "/rest/tasks", listKey: "tasks", title: (record) => tasks.title(record) },
  note: { label: "Note", path: "/rest/notes", listKey: "notes", title: (record) => notes.title(record) },
  member: { label: "Member", path: "", listKey: "", title: (record) => text(record.userEmail) || "Member" },
};
