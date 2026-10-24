import type { GenericRecord } from "./resources";

function obj(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : {};
}

export function recordValue(record: GenericRecord, key: string): unknown {
  switch (key) {
    case "firstName":
      return obj(record.name).firstName;
    case "lastName":
      return obj(record.name).lastName;
    case "primaryEmail":
      return obj(record.emails).primaryEmail;
    case "additionalEmails":
      return obj(record.emails).additionalEmails;
    case "domain":
      return obj(record.domainName).primaryLinkUrl;
    case "linkedinUrl":
      return obj(record.linkedinLink).primaryLinkUrl;
    case "phone":
      return obj(record.phones).primaryPhoneNumber;
    case "bodyMarkdown":
      return obj(record.bodyV2).markdown;
    case "addressStreet1":
      return obj(record.address).addressStreet1;
    case "addressStreet2":
      return obj(record.address).addressStreet2;
    case "addressCity":
      return obj(record.address).addressCity;
    case "addressPostcode":
      return obj(record.address).addressPostcode;
    case "addressState":
      return obj(record.address).addressState;
    case "addressCountry":
      return obj(record.address).addressCountry;
    case "currencyCode":
      return obj(record.amount).currencyCode ?? obj(record.annualRevenue).currencyCode;
    default:
      return record[key];
  }
}
