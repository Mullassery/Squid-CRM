import { ApiError } from "./api";

const API_BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? "/api";

interface GraphQLErrorShape {
  message: string;
  path?: (string | number)[];
  extensions?: { code?: string };
}

interface GraphQLResponse<T> {
  data?: T | null;
  errors?: GraphQLErrorShape[];
}

export async function gql<T>(
  query: string,
  variables: Record<string, unknown> = {},
  endpoint: "/graphql" | "/metadata" = "/metadata",
): Promise<T> {
  const response = await fetch(`${API_BASE}${endpoint}`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({ query, variables }),
  });

  const payload = (await response.json().catch(() => null)) as
    | GraphQLResponse<T>
    | null;

  if (!response.ok) {
    throw new ApiError(
      payload?.errors?.[0]?.message ?? `Request failed (${response.status})`,
      response.status,
      payload?.errors?.[0]?.extensions?.code,
      payload,
    );
  }

  if (payload?.errors?.length) {
    const first = payload.errors[0];
    throw new ApiError(
      first.message,
      400,
      first.extensions?.code,
      payload.errors,
    );
  }

  if (!payload?.data) {
    throw new ApiError("The server returned no data.", 500, undefined, payload);
  }

  return payload.data;
}

export const fragments = {
  role: `
    fragment RoleFields on Role {
      id
      label
      description
      icon
      canUpdateAllSettings
      canAccessAllTools
      isEditable
      canReadAllObjectRecords
      canUpdateAllObjectRecords
      canSoftDeleteAllObjectRecords
      canDestroyAllObjectRecords
      canBeAssignedToUsers
      canBeAssignedToAgents
      canBeAssignedToApiKeys
      applicationId
      universalIdentifier
      permissionFlags { id roleId flag }
    }
  `,
  member: `
    fragment MemberFields on WorkspaceMember {
      id
      createdAt
      updatedAt
      position
      name { firstName lastName }
      userEmail
      avatarUrl
      jobTitle
      roles { id label }
      userId
    }
  `,
};

export const queries = {
  currentUser: `
    query CurrentUser {
      currentUser {
        id
        firstName
        lastName
        email
        currentWorkspace {
          id
          displayName
          subdomain
          workspaceMembersCount
        }
        workspaceMember {
          id
          name { firstName lastName }
          userEmail
        }
      }
    }
  `,
  members: `
    ${fragments.member}
    query Members($first: Int) {
      workspaceMembers(first: $first) {
        edges { node { ...MemberFields } }
        pageInfo { hasNextPage endCursor }
      }
    }
  `,
  roles: `
    ${fragments.role}
    query Roles { getRoles { ...RoleFields } }
  `,
  apiKeyRoles: `
    ${fragments.role}
    query ApiKeyRoles { getApiKeyRoles { ...RoleFields } }
  `,
  permissionFlags: `
    query PermissionFlags {
      getPermissionFlags {
        id
        universalIdentifier
        key
        label
        description
        icon
        permissionType
        applicationId
        createdAt
        updatedAt
      }
    }
  `,
  apiKeys: `
    ${fragments.role}
    query ApiKeys {
      apiKeys {
        id
        name
        expiresAt
        revokedAt
        createdAt
        updatedAt
        role { ...RoleFields }
      }
    }
  `,
  webhooks: `
    query Webhooks {
      webhooks {
        id
        targetUrl
        operations
        description
        secret
        createdAt
        updatedAt
        deletedAt
      }
    }
  `,
  webhookDeliveries: `
    query WebhookDeliveries($webhookId: UUID) {
      webhookDeliveries(webhookId: $webhookId, limit: 100) {
        id
        webhookId
        event
        eventId
        state
        attempts
        lastStatusCode
        lastError
        targetUrl
        createdAt
        updatedAt
        completedAt
      }
    }
  `,
  jobs: `
    query Jobs($state: String) {
      jobs(jobState: $state, limit: 100) {
        id
        workspaceId
        kind
        payload
        state
        attempts
        maxAttempts
        runAt
        lastError
        createdAt
        updatedAt
        completedAt
      }
    }
  `,
  objects: `
    query Objects($first: Int) {
      objects(paging: { first: $first }) {
        edges {
          node {
            id
            universalIdentifier
            nameSingular
            namePlural
            labelSingular
            labelPlural
            description
            icon
            color
            isActive
            isSystem
            isRemote
            isUIEditable
            isUICreatable
            isSearchable
            fieldsList {
              id
              universalIdentifier
              type
              name
              label
              description
              icon
              isActive
              isSystem
              isUIEditable
              writability
              isNullable
              isUnique
              isSearchable
              isAuditLogged
              defaultValue
              options
              settings
              objectMetadataId
              isLabelSyncedWithName
              morphId
              createdAt
              updatedAt
              applicationId
              relation {
                type
                sourceObjectMetadata { id nameSingular namePlural }
                targetObjectMetadata { id nameSingular namePlural }
                sourceFieldMetadata { id name }
                targetFieldMetadata { id name }
              }
              morphRelations {
                type
                sourceObjectMetadata { id nameSingular namePlural }
                targetObjectMetadata { id nameSingular namePlural }
                sourceFieldMetadata { id name }
                targetFieldMetadata { id name }
              }
            }
            indexMetadataList {
              id
              name
              isCustom
              isUnique
              indexWhereClause
              indexType
              indexFieldMetadataList { id fieldMetadataId order }
              createdAt
              updatedAt
            }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  `,
  object: `
    query Object($id: UUID!) {
      object(id: $id) {
        id
        nameSingular
        namePlural
        labelSingular
        labelPlural
        description
        icon
        color
        isActive
        isSystem
        isRemote
        isUIEditable
        isUICreatable
        isSearchable
        fieldsList {
          id
          universalIdentifier
          type
          name
          label
          description
          icon
          isActive
          isSystem
          isUIEditable
          writability
          isNullable
          isUnique
          isSearchable
          isAuditLogged
          defaultValue
          options
          settings
          objectMetadataId
          isLabelSyncedWithName
          morphId
          createdAt
          updatedAt
          applicationId
          relation {
            type
            sourceObjectMetadata { id nameSingular namePlural }
            targetObjectMetadata { id nameSingular namePlural }
            sourceFieldMetadata { id name }
            targetFieldMetadata { id name }
          }
          morphRelations {
            type
            sourceObjectMetadata { id nameSingular namePlural }
            targetObjectMetadata { id nameSingular namePlural }
            sourceFieldMetadata { id name }
            targetFieldMetadata { id name }
          }
        }
        indexMetadataList {
          id
          name
          isCustom
          isUnique
          indexWhereClause
          indexType
          indexFieldMetadataList { id fieldMetadataId order }
          createdAt
          updatedAt
        }
      }
    }
  `,
  views: `
    query Views($objectMetadataId: String) {
      getViews(objectMetadataId: $objectMetadataId) {
        id
        name
        objectMetadataId
        type
        key
        icon
        position
        isCustom
        viewFields {
          id
          fieldMetadataId
          isVisible
          size
          position
          viewId
          viewFieldGroupId
          aggregateOperation
        }
        viewFilters {
          id
          fieldMetadataId
          operand
          value
          subFieldName
          viewId
          viewFilterGroupId
          positionInViewFilterGroup
        }
        viewSorts {
          id
          fieldMetadataId
          direction
          subFieldName
          viewId
        }
        viewGroups {
          id
          isVisible
          fieldValue
          position
          viewId
        }
      }
    }
  `,
  view: `
    query View($id: String!) {
      getView(id: $id) {
        id
        name
        objectMetadataId
        type
        key
        icon
        position
        isCustom
        viewFields {
          id
          fieldMetadataId
          isVisible
          size
          position
          viewId
          viewFieldGroupId
          aggregateOperation
        }
        viewFilters {
          id
          fieldMetadataId
          operand
          value
          subFieldName
          viewId
          viewFilterGroupId
          positionInViewFilterGroup
        }
        viewSorts {
          id
          fieldMetadataId
          direction
          subFieldName
          viewId
        }
        viewGroups {
          id
          isVisible
          fieldValue
          position
          viewId
        }
      }
    }
  `,
  search: `
    query Search($q: String!, $limit: Int) {
      search(searchInput: $q, limit: $limit) {
        edges {
          node {
            recordId
            objectNameSingular
            objectLabelSingular
            label
            imageUrl
            tsRank
            tsRankCD
          }
          cursor
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  `,
  customRecords: `
    query CustomRecords($name: String!, $limit: Int, $offset: Int) {
      customObjectRecords(objectNameSingular: $name, limit: $limit, offset: $offset)
    }
  `,
  customRecord: `
    query CustomRecord($name: String!, $id: UUID!) {
      customObjectRecord(objectNameSingular: $name, id: $id)
    }
  `,
};

export const mutations = {
  fileUpload: `
    mutation FileUpload($filename: String!, $size: Float!, $folder: FileFolder!) {
      createFileUpload(filename: $filename, size: $size, fileFolder: $folder) {
        fileId
        uploadUrl
        contentType
        expiresAt
      }
    }
  `,
  completeFileUpload: `
    mutation CompleteFileUpload($fileId: String!) {
      completeFileUpload(fileId: $fileId) {
        id
        path
        size
        createdAt
        url
      }
    }
  `,
  createApiKey: `
    mutation CreateApiKey($input: CreateApiKeyInput!) {
      createApiKey(input: $input) { id name expiresAt }
    }
  `,
  generateApiKeyToken: `
    mutation GenerateApiKeyToken($id: UUID!, $expiresAt: String!) {
      generateApiKeyToken(apiKeyId: $id, expiresAt: $expiresAt) { token }
    }
  `,
  revokeApiKey: `
    mutation RevokeApiKey($input: RevokeApiKeyInput!) {
      revokeApiKey(input: $input) { id name revokedAt }
    }
  `,
  assignRoleToApiKey: `
    mutation AssignRoleToApiKey($apiKeyId: UUID!, $roleId: UUID!) {
      assignRoleToApiKey(apiKeyId: $apiKeyId, roleId: $roleId)
    }
  `,
  createRole: `
    mutation CreateRole($input: CreateRoleInput!) {
      createOneRole(createRoleInput: $input) { id label }
    }
  `,
  updateRole: `
    mutation UpdateRole($input: UpdateRoleInput!) {
      updateOneRole(updateRoleInput: $input) { id label }
    }
  `,
  deleteRole: `
    mutation DeleteRole($roleId: UUID!) {
      deleteOneRole(roleId: $roleId)
    }
  `,
  updateMemberRole: `
    ${fragments.member}
    mutation UpdateMemberRole($workspaceMemberId: UUID!, $roleId: UUID!) {
      updateWorkspaceMemberRole(workspaceMemberId: $workspaceMemberId, roleId: $roleId) {
        ...MemberFields
      }
    }
  `,
  upsertPermissionFlags: `
    mutation UpsertPermissionFlags($input: UpsertPermissionFlagsInput!) {
      upsertPermissionFlags(upsertPermissionFlagsInput: $input) { id roleId flag }
    }
  `,
  createWebhook: `
    mutation CreateWebhook($input: CreateWebhookInput!) {
      createWebhook(input: $input) { id targetUrl operations description }
    }
  `,
  updateWebhook: `
    mutation UpdateWebhook($input: UpdateWebhookInput!) {
      updateWebhook(input: $input) { id targetUrl operations description }
    }
  `,
  deleteWebhook: `
    mutation DeleteWebhook($id: UUID!) {
      deleteWebhook(id: $id) { id }
    }
  `,
  replayWebhookDelivery: `
    mutation ReplayWebhookDelivery($id: UUID!) {
      replayWebhookDelivery(id: $id)
    }
  `,
  createView: `
    mutation CreateView($input: CreateViewInput!) {
      createView(input: $input) { id name }
    }
  `,
  updateView: `
    mutation UpdateView($id: String!, $input: UpdateViewInput!) {
      updateView(id: $id, input: $input) { id name }
    }
  `,
  deleteView: `
    mutation DeleteView($id: String!) { deleteView(id: $id) }
  `,
  updateViewField: `
    mutation UpdateViewField($input: UpdateViewFieldInput!) {
      updateViewField(input: $input) { id isVisible }
    }
  `,
  createOneObject: `
    mutation CreateOneObject($input: CreateOneObjectInput!) {
      createOneObject(input: $input) { id nameSingular }
    }
  `,
  updateOneObject: `
    mutation UpdateOneObject($input: UpdateOneObjectInput!) {
      updateOneObject(input: $input) { id nameSingular labelSingular }
    }
  `,
  deleteOneObject: `
    mutation DeleteOneObject($input: DeleteOneObjectInput!) {
      deleteOneObject(input: $input) { id }
    }
  `,
  createOneField: `
    mutation CreateOneField($input: CreateOneFieldMetadataInput!) {
      createOneField(input: $input) { id name label }
    }
  `,
  updateOneField: `
    mutation UpdateOneField($input: UpdateOneFieldMetadataInput!) {
      updateOneField(input: $input) { id name label }
    }
  `,
  deleteOneField: `
    mutation DeleteOneField($input: DeleteOneFieldInput!) {
      deleteOneField(input: $input) { id }
    }
  `,
  createOneIndex: `
    mutation CreateOneIndex($input: CreateOneIndexInput!) {
      createOneIndex(input: $input) { id name indexType }
    }
  `,
  deleteOneIndex: `
    mutation DeleteOneIndex($input: DeleteOneIndexInput!) {
      deleteOneIndex(input: $input) { id }
    }
  `,
  createCustomRecord: `
    mutation CreateCustomRecord($name: String!, $data: JSON!) {
      createCustomObjectRecord(objectNameSingular: $name, data: $data)
    }
  `,
  updateCustomRecord: `
    mutation UpdateCustomRecord($name: String!, $id: UUID!, $data: JSON!) {
      updateCustomObjectRecord(objectNameSingular: $name, id: $id, data: $data)
    }
  `,
  deleteCustomRecord: `
    mutation DeleteCustomRecord($name: String!, $id: UUID!, $softDelete: Boolean!) {
      deleteCustomObjectRecord(objectNameSingular: $name, id: $id, softDelete: $softDelete)
    }
  `,
  enqueueJob: `
    mutation EnqueueJob($kind: String!, $payload: JSON, $maxAttempts: Int) {
      enqueueJob(kind: $kind, payload: $payload, maxAttempts: $maxAttempts) { id kind state }
    }
  `,
  runJob: `
    mutation RunJob($id: UUID!) {
      runJob(id: $id) { id kind state attempts lastError }
    }
  `,
};
