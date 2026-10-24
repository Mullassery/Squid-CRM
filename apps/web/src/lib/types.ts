export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type JsonObject = { [key: string]: JsonValue };

export interface PageInfo {
  hasNextPage: boolean;
  hasPreviousPage: boolean;
  startCursor: string | null;
  endCursor: string | null;
}

export interface FindManyResponse<T> {
  data: Record<string, T[]>;
  totalCount: number;
  pageInfo: PageInfo;
}

export interface Actor {
  source: string;
  workspaceMemberId: string | null;
  name: string;
  context: JsonValue;
}

export interface FullName {
  firstName: string;
  lastName: string;
}

export interface Emails {
  primaryEmail: string;
  additionalEmails: string[];
}

export interface Links {
  primaryLinkLabel: string;
  primaryLinkUrl: string;
  secondaryLinks: JsonValue[];
}

export interface Phones {
  primaryPhoneNumber: string;
  primaryPhoneCountryCode: string;
  primaryPhoneCallingCode: string;
  additionalPhones: JsonValue[];
}

export interface Currency {
  amountMicros: number | null;
  currencyCode: string;
}

export interface Address {
  addressStreet1: string | null;
  addressStreet2: string | null;
  addressCity: string | null;
  addressPostcode: string | null;
  addressState: string | null;
  addressCountry: string | null;
  addressLat: number | null;
  addressLng: number | null;
}

export interface RichText {
  blocknote: string | null;
  markdown: string | null;
}

export interface BaseRecord {
  id: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  position: number;
  createdBy: Actor;
  updatedBy: Actor;
  searchVector: string | null;
}

export interface Person extends BaseRecord {
  name: FullName;
  emails: Emails;
  linkedinLink: Links;
  jobTitle: string | null;
  phones: Phones;
  avatarUrl: string;
  avatarFile: JsonValue | null;
  companyId: string | null;
}

export interface Company extends BaseRecord {
  name: string | null;
  domainName: Links;
  address: Address;
  linkedinLink: Links;
  annualRevenue: Currency;
  accountOwnerId: string | null;
}

export interface Opportunity extends BaseRecord {
  name: string | null;
  amount: Currency;
  closeDate: string | null;
  stage: string;
  companyId: string | null;
  pointOfContactId: string | null;
  ownerId: string | null;
}

export interface Task extends BaseRecord {
  title: string | null;
  bodyV2: RichText;
  dueAt: string | null;
  status: string | null;
  assigneeId: string | null;
}

export interface Note extends BaseRecord {
  title: string | null;
  bodyV2: RichText;
}

export interface TargetRecord extends BaseRecord {
  targetCompanyId: string | null;
  targetOpportunityId: string | null;
  targetPersonId: string | null;
}

export interface TaskTarget extends TargetRecord {
  taskId: string;
}

export interface NoteTarget extends TargetRecord {
  noteId: string;
}

export type CrmRecord =
  | Person
  | Company
  | Opportunity
  | Task
  | Note
  | TaskTarget
  | NoteTarget;

export interface SessionUser {
  id: string;
  email: string;
  createdAt: string;
}

export interface SessionWorkspace {
  id: string;
  displayName: string;
  subdomain: string;
  createdAt: string;
}

export interface Session {
  user: SessionUser;
  workspace: SessionWorkspace;
  role: string;
}

export interface AuthTokens {
  accessOrWorkspaceAgnosticToken: { token: string; expiresAt: string };
  refreshToken: { token: string; expiresAt: string };
}

export interface AuthResponse {
  tokens: AuthTokens;
  user: SessionUser;
  workspace: SessionWorkspace;
}

export interface Role {
  id: string;
  label: string;
  description: string | null;
  icon: string | null;
  canUpdateAllSettings: boolean;
  canAccessAllTools: boolean;
  isEditable: boolean;
  canReadAllObjectRecords: boolean;
  canUpdateAllObjectRecords: boolean;
  canSoftDeleteAllObjectRecords: boolean;
  canDestroyAllObjectRecords: boolean;
  canBeAssignedToUsers: boolean;
  canBeAssignedToAgents: boolean;
  canBeAssignedToApiKeys: boolean;
  applicationId: string;
  universalIdentifier: string | null;
  permissionFlags: { id: string; roleId: string; flag: string }[] | null;
}

export interface PermissionFlag {
  id: string;
  universalIdentifier: string;
  key: string;
  label: string;
  description: string | null;
  icon: string | null;
  permissionType: string;
  applicationId: string;
  createdAt: string;
  updatedAt: string;
}

export interface WorkspaceMember {
  id: string;
  createdAt: string;
  updatedAt: string;
  position: number;
  name: FullName;
  userEmail: string;
  avatarUrl: string | null;
  jobTitle: string | null;
  roles: Role[] | null;
  userId: string;
}

export interface ApiKeyRecord {
  id: string;
  name: string;
  expiresAt: string;
  revokedAt: string | null;
  createdAt: string;
  updatedAt: string;
  role: Role;
}

export interface WebhookRecord {
  id: string;
  targetUrl: string;
  operations: string[];
  description: string | null;
  secret: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface WebhookDelivery {
  id: string;
  webhookId: string;
  event: string;
  eventId: string;
  state: string;
  attempts: number;
  lastStatusCode: number | null;
  lastError: string | null;
  targetUrl: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface JobRecord {
  id: string;
  workspaceId: string | null;
  kind: string;
  payload: JsonValue;
  state: string;
  attempts: number;
  maxAttempts: number;
  runAt: string;
  lastError: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface ViewRecord {
  id: string;
  name: string;
  objectMetadataId: string;
  type: string;
  key: string | null;
  icon: string;
  position: number;
  isCustom: boolean;
  viewFields: ViewField[];
  viewFilters: ViewFilter[];
  viewSorts: ViewSort[];
  viewGroups: ViewGroup[];
}

export interface ViewField {
  id: string;
  fieldMetadataId: string;
  isVisible: boolean;
  size: number;
  position: number;
  viewId: string;
  viewFieldGroupId: string | null;
  aggregateOperation: string | null;
}

export interface ViewFilter {
  id: string;
  fieldMetadataId: string;
  operand: string;
  value: JsonValue;
  subFieldName: string | null;
  viewId: string;
  viewFilterGroupId: string | null;
  positionInViewFilterGroup: number | null;
}

export interface ViewSort {
  id: string;
  fieldMetadataId: string;
  direction: string;
  subFieldName: string | null;
  viewId: string;
}

export interface ViewGroup {
  id: string;
  isVisible: boolean;
  fieldValue: string;
  position: number;
  viewId: string;
}

export interface FieldMetadata {
  id: string;
  universalIdentifier: string;
  type: string;
  name: string;
  label: string;
  description: string | null;
  icon: string | null;
  isActive: boolean;
  isSystem: boolean;
  isUIEditable: boolean;
  writability: string;
  isNullable: boolean;
  isUnique: boolean;
  isSearchable: boolean;
  isAuditLogged: boolean;
  defaultValue: JsonValue | null;
  options: JsonValue | null;
  settings: JsonValue | null;
  objectMetadataId: string;
  isLabelSyncedWithName: boolean;
  morphId: string | null;
  createdAt: string;
  updatedAt: string;
  applicationId: string;
  relation: RelationMetadata | null;
  morphRelations: RelationMetadata[];
}

export interface ObjectSummary {
  id: string;
  nameSingular: string;
  namePlural: string;
}

export interface RelationMetadata {
  type: string;
  sourceObjectMetadata: ObjectSummary;
  targetObjectMetadata: ObjectSummary;
  sourceFieldMetadata: { id: string; name: string };
  targetFieldMetadata: { id: string; name: string };
}

export interface IndexMetadata {
  id: string;
  name: string;
  isCustom: boolean;
  isUnique: boolean;
  indexWhereClause: string | null;
  indexType: string;
  indexFieldMetadataList: { id: string; fieldMetadataId: string; order: number }[];
  createdAt: string;
  updatedAt: string;
}

export interface ObjectMetadata {
  id: string;
  universalIdentifier: string;
  nameSingular: string;
  namePlural: string;
  labelSingular: string;
  labelPlural: string;
  description: string | null;
  icon: string | null;
  color: string | null;
  isActive: boolean;
  isSystem: boolean;
  isRemote: boolean;
  isUIEditable: boolean;
  isUICreatable: boolean;
  isSearchable: boolean;
  fieldsList: FieldMetadata[];
  indexMetadataList: IndexMetadata[];
}

export interface SearchRecord {
  recordId: string;
  objectNameSingular: string;
  objectLabelSingular: string;
  label: string;
  imageUrl: string | null;
  tsRank: number;
  tsRankCD: number;
}

export interface SearchResult {
  edges: { node: SearchRecord; cursor: string }[];
  pageInfo: { hasNextPage: boolean; endCursor: string | null };
}

export interface CurrentUser {
  id: string;
  firstName: string;
  lastName: string;
  email: string;
  currentWorkspace: {
    id: string;
    displayName: string | null;
    subdomain: string;
    workspaceMembersCount: number | null;
  } | null;
  workspaceMember: {
    id: string;
    name: FullName;
    userEmail: string;
  } | null;
}

export interface CustomRecord {
  id: string;
  createdAt?: string;
  updatedAt?: string;
  [key: string]: JsonValue | undefined;
}
