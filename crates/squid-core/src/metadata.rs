//! Workspace object/field metadata models.
//!
//! Phase 5 slice: the shapes mirror the subset of the upstream metadata schema
//! consumed by `twenty-front`'s `ObjectMetadataFields` fragment. Relation,
//! index and search-field metadata are modelled but standard objects currently
//! expose empty lists.

use async_graphql::{ComplexObject, Enum, InputObject, Json, SimpleObject};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "FieldMetadataType")]
pub enum FieldMetadataType {
    Actor,
    Address,
    Array,
    Boolean,
    Currency,
    Date,
    DateTime,
    Emails,
    Files,
    FullName,
    Links,
    MorphRelation,
    MultiSelect,
    Number,
    Numeric,
    Phones,
    Position,
    Rating,
    RawJson,
    Relation,
    RichText,
    Select,
    Text,
    TsVector,
    Uuid,
}

impl FieldMetadataType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actor => "ACTOR",
            Self::Address => "ADDRESS",
            Self::Array => "ARRAY",
            Self::Boolean => "BOOLEAN",
            Self::Currency => "CURRENCY",
            Self::Date => "DATE",
            Self::DateTime => "DATE_TIME",
            Self::Emails => "EMAILS",
            Self::Files => "FILES",
            Self::FullName => "FULL_NAME",
            Self::Links => "LINKS",
            Self::MorphRelation => "MORPH_RELATION",
            Self::MultiSelect => "MULTI_SELECT",
            Self::Number => "NUMBER",
            Self::Numeric => "NUMERIC",
            Self::Phones => "PHONES",
            Self::Position => "POSITION",
            Self::Rating => "RATING",
            Self::RawJson => "RAW_JSON",
            Self::Relation => "RELATION",
            Self::RichText => "RICH_TEXT",
            Self::Select => "SELECT",
            Self::Text => "TEXT",
            Self::TsVector => "TS_VECTOR",
            Self::Uuid => "UUID",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ACTOR" => Self::Actor,
            "ADDRESS" => Self::Address,
            "ARRAY" => Self::Array,
            "BOOLEAN" => Self::Boolean,
            "CURRENCY" => Self::Currency,
            "DATE" => Self::Date,
            "DATE_TIME" => Self::DateTime,
            "EMAILS" => Self::Emails,
            "FILES" => Self::Files,
            "FULL_NAME" => Self::FullName,
            "LINKS" => Self::Links,
            "MORPH_RELATION" => Self::MorphRelation,
            "MULTI_SELECT" => Self::MultiSelect,
            "NUMBER" => Self::Number,
            "NUMERIC" => Self::Numeric,
            "PHONES" => Self::Phones,
            "POSITION" => Self::Position,
            "RATING" => Self::Rating,
            "RAW_JSON" => Self::RawJson,
            "RELATION" => Self::Relation,
            "RICH_TEXT" => Self::RichText,
            "SELECT" => Self::Select,
            "TEXT" => Self::Text,
            "TS_VECTOR" => Self::TsVector,
            "UUID" => Self::Uuid,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "MetadataWritability")]
pub enum MetadataWritability {
    Open,
    Application,
    System,
}

impl MetadataWritability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Application => "APPLICATION",
            Self::System => "SYSTEM",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "OPEN" => Self::Open,
            "APPLICATION" => Self::Application,
            "SYSTEM" => Self::System,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "MetadataReadability")]
pub enum MetadataReadability {
    Open,
    Private,
    Inherited,
    Application,
    System,
}

impl MetadataReadability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Private => "PRIVATE",
            Self::Inherited => "INHERITED",
            Self::Application => "APPLICATION",
            Self::System => "SYSTEM",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "OPEN" => Self::Open,
            "PRIVATE" => Self::Private,
            "INHERITED" => Self::Inherited,
            "APPLICATION" => Self::Application,
            "SYSTEM" => Self::System,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "ObjectSharingReach")]
pub enum ObjectSharingReach {
    RoleAccess,
    Workspace,
}

impl ObjectSharingReach {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoleAccess => "ROLE_ACCESS",
            Self::Workspace => "WORKSPACE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ROLE_ACCESS" => Self::RoleAccess,
            "WORKSPACE" => Self::Workspace,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "ObjectOpenRecordIn")]
pub enum ObjectOpenRecordIn {
    SidePanel,
    RecordPage,
    UserChoice,
}

impl ObjectOpenRecordIn {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SidePanel => "SIDE_PANEL",
            Self::RecordPage => "RECORD_PAGE",
            Self::UserChoice => "USER_CHOICE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "SIDE_PANEL" => Self::SidePanel,
            "RECORD_PAGE" => Self::RecordPage,
            "USER_CHOICE" => Self::UserChoice,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[graphql(name = "IndexType")]
pub enum IndexType {
    Btree,
    Gin,
}

impl IndexType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Btree => "BTREE",
            Self::Gin => "GIN",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "BTREE" => Some(Self::Btree),
            "GIN" => Some(Self::Gin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, InputObject)]
pub struct CursorPagingInput {
    pub before: Option<String>,
    pub after: Option<String>,
    pub first: Option<i32>,
    pub last: Option<i32>,
}

#[derive(Debug, Clone, Default, InputObject)]
pub struct UuidFilterComparison {
    pub eq: Option<Uuid>,
    pub neq: Option<Uuid>,
}

#[derive(Debug, Clone, Default, InputObject)]
pub struct BooleanFilterComparison {
    pub eq: Option<bool>,
    pub neq: Option<bool>,
}

#[derive(Debug, Clone, Default, InputObject)]
pub struct ObjectFilterInput {
    pub id: Option<UuidFilterComparison>,
    pub is_active: Option<BooleanFilterComparison>,
    pub is_system: Option<BooleanFilterComparison>,
}

#[derive(Debug, Clone, Default, InputObject)]
pub struct FieldFilterInput {
    pub id: Option<UuidFilterComparison>,
    pub is_active: Option<BooleanFilterComparison>,
    pub is_system: Option<BooleanFilterComparison>,
    pub object_metadata_id: Option<UuidFilterComparison>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct SearchFieldMetadata {
    pub id: Uuid,
    pub field_metadata_id: Uuid,
    pub ts_vector_field_metadata_id: Uuid,
    pub position: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "IndexField")]
pub struct IndexFieldMetadata {
    pub id: Uuid,
    pub field_metadata_id: Uuid,
    pub order: f64,
    pub sub_field_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, InputObject)]
pub struct CreateIndexFieldInput {
    pub field_metadata_id: Uuid,
    pub sub_field_name: Option<String>,
}

#[derive(Debug, Clone, InputObject)]
pub struct CreateIndexInput {
    pub object_metadata_id: Uuid,
    pub fields: Vec<CreateIndexFieldInput>,
    #[graphql(default_with = "IndexType::Btree")]
    pub index_type: IndexType,
}

#[derive(Debug, Clone, InputObject)]
pub struct CreateOneIndexInput {
    pub index: CreateIndexInput,
}

#[derive(Debug, Clone, InputObject)]
pub struct DeleteOneIndexInput {
    pub id: Uuid,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Index")]
pub struct IndexMetadata {
    pub id: Uuid,
    pub name: String,
    pub is_custom: bool,
    pub is_unique: bool,
    pub index_where_clause: Option<String>,
    pub index_type: IndexType,
    pub index_field_metadata_list: Vec<IndexFieldMetadata>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
#[graphql(name = "RelationType")]
pub enum RelationType {
    OneToMany,
    ManyToOne,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Relation")]
pub struct RelationMetadata {
    #[graphql(name = "type")]
    pub relation_type: RelationType,
    pub source_object_metadata: ObjectSummary,
    pub target_object_metadata: ObjectSummary,
    pub source_field_metadata: FieldSummary,
    pub target_field_metadata: FieldSummary,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ObjectSummary {
    pub id: Uuid,
    pub name_singular: String,
    pub name_plural: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct FieldSummary {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct FieldEdge {
    pub node: FieldMetadata,
    pub cursor: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct FieldConnection {
    pub page_info: crate::PageInfo,
    pub edges: Vec<FieldEdge>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ObjectFieldsConnection")]
pub struct ObjectFieldsConnection {
    pub page_info: crate::PageInfo,
    pub edges: Vec<FieldEdge>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ObjectEdge {
    pub node: ObjectMetadata,
    pub cursor: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ObjectConnection {
    pub page_info: crate::PageInfo,
    pub edges: Vec<ObjectEdge>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex, name = "Object")]
pub struct ObjectMetadata {
    pub id: Uuid,
    pub universal_identifier: String,
    pub name_singular: String,
    pub name_plural: String,
    pub label_singular: String,
    pub label_plural: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub color: Option<String>,
    pub is_remote: bool,
    pub is_active: bool,
    pub is_system: bool,
    #[graphql(name = "isUIEditable")]
    pub is_ui_editable: bool,
    #[graphql(name = "isUICreatable")]
    pub is_ui_creatable: bool,
    pub is_searchable: bool,
    pub open_record_in: ObjectOpenRecordIn,
    pub readability: MetadataReadability,
    pub sharing_reach: ObjectSharingReach,
    pub writability: MetadataWritability,
    pub application_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub label_identifier_field_metadata_id: Option<Uuid>,
    pub image_identifier_field_metadata_id: Option<Uuid>,
    pub is_label_synced_with_name: bool,
    pub duplicate_criteria: Option<Vec<Vec<String>>>,
    #[graphql(skip)]
    pub fields: Vec<FieldMetadata>,
    #[graphql(skip)]
    pub index_metadata: Vec<IndexMetadata>,
    #[graphql(skip)]
    pub search_field_metadata: Vec<SearchFieldMetadata>,
}

#[ComplexObject]
impl ObjectMetadata {
    async fn fields_list(&self) -> Vec<FieldMetadata> {
        self.fields.clone()
    }

    async fn fields(&self, paging: Option<CursorPagingInput>) -> ObjectFieldsConnection {
        object_fields_connection(&self.fields, paging)
    }

    async fn index_metadata_list(&self) -> Vec<IndexMetadata> {
        self.index_metadata.clone()
    }

    async fn search_field_metadata_list(&self) -> Vec<SearchFieldMetadata> {
        self.search_field_metadata.clone()
    }
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex, name = "Field")]
pub struct FieldMetadata {
    pub id: Uuid,
    pub universal_identifier: String,
    #[graphql(name = "type")]
    pub field_type: FieldMetadataType,
    pub name: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub is_active: bool,
    pub is_system: bool,
    #[graphql(name = "isUIEditable")]
    pub is_ui_editable: bool,
    pub writability: MetadataWritability,
    pub is_nullable: bool,
    pub is_unique: bool,
    pub is_searchable: bool,
    pub is_audit_logged: bool,
    pub default_value: Option<Json<Value>>,
    pub options: Option<Json<Value>>,
    pub settings: Option<Json<Value>>,
    pub object_metadata_id: Uuid,
    pub is_label_synced_with_name: bool,
    pub morph_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub application_id: Uuid,
    #[graphql(skip)]
    pub relation: Option<RelationMetadata>,
    #[graphql(skip)]
    pub morph_relations: Vec<RelationMetadata>,
}

#[ComplexObject]
impl FieldMetadata {
    async fn relation(&self) -> Option<RelationMetadata> {
        self.relation.clone()
    }

    async fn morph_relations(&self) -> Vec<RelationMetadata> {
        self.morph_relations.clone()
    }

    async fn object(&self) -> Option<ObjectSummary> {
        None
    }
}

pub fn object_fields_connection(
    fields: &[FieldMetadata],
    paging: Option<CursorPagingInput>,
) -> ObjectFieldsConnection {
    let paging = paging.unwrap_or(CursorPagingInput {
        before: None,
        after: None,
        first: None,
        last: None,
    });
    let mut items = fields.to_vec();
    if let Some(after) = paging.after.as_deref() {
        if let Some(position) = items.iter().position(|field| field.id.to_string() == after) {
            items.drain(..=position);
        }
    }
    if let Some(before) = paging.before.as_deref() {
        if let Some(position) = items
            .iter()
            .position(|field| field.id.to_string() == before)
        {
            items.truncate(position);
        }
    }
    let has_next_page = paging
        .first
        .and_then(|first| usize::try_from(first).ok())
        .is_some_and(|first| items.len() > first);
    if let Some(first) = paging.first.and_then(|first| usize::try_from(first).ok()) {
        items.truncate(first);
    }
    if let Some(last) = paging.last.and_then(|last| usize::try_from(last).ok()) {
        if items.len() > last {
            items.drain(..items.len() - last);
        }
    }
    let edges: Vec<FieldEdge> = items
        .into_iter()
        .map(|node| FieldEdge {
            cursor: node.id.to_string(),
            node,
        })
        .collect();
    let page_info = crate::PageInfo {
        has_next_page,
        has_previous_page: false,
        start_cursor: edges.first().map(|edge| edge.cursor.clone()),
        end_cursor: edges.last().map(|edge| edge.cursor.clone()),
    };
    ObjectFieldsConnection { page_info, edges }
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateObjectInput")]
pub struct CreateObjectMetadataInput {
    pub name_singular: String,
    pub name_plural: String,
    pub label_singular: String,
    pub label_plural: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    #[graphql(default)]
    pub skip_name_field: bool,
    #[serde(default)]
    #[graphql(default)]
    pub is_remote: bool,
    #[serde(default)]
    #[graphql(default)]
    pub is_label_synced_with_name: bool,
    #[serde(default)]
    pub primary_key_column_type: Option<String>,
    #[serde(default)]
    pub primary_key_field_metadata_settings: Option<Json<Value>>,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateObjectPayload")]
pub struct UpdateObjectMetadataInput {
    #[serde(default)]
    pub label_singular: Option<String>,
    #[serde(default)]
    pub label_plural: Option<String>,
    #[serde(default)]
    pub name_singular: Option<String>,
    #[serde(default)]
    pub name_plural: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub label_identifier_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub image_identifier_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub is_label_synced_with_name: Option<bool>,
    #[serde(default)]
    pub is_searchable: Option<bool>,
    #[serde(default)]
    pub open_record_in: Option<ObjectOpenRecordIn>,
    #[serde(default)]
    pub readability: Option<MetadataReadability>,
    #[serde(default)]
    pub sharing_reach: Option<ObjectSharingReach>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateFieldInput")]
pub struct CreateFieldMetadataInput {
    #[serde(rename = "type")]
    #[graphql(name = "type")]
    pub field_type: FieldMetadataType,
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub is_system: Option<bool>,
    #[serde(default)]
    pub is_ui_editable: Option<bool>,
    #[serde(default)]
    pub is_ui_read_only: Option<bool>,
    #[serde(default)]
    pub is_nullable: Option<bool>,
    #[serde(default)]
    pub is_unique: Option<bool>,
    #[serde(default)]
    pub is_searchable: Option<bool>,
    #[serde(default)]
    pub is_audit_logged: Option<bool>,
    #[serde(default)]
    pub default_value: Option<Json<Value>>,
    #[serde(default)]
    pub options: Option<Json<Value>>,
    #[serde(default)]
    pub settings: Option<Json<Value>>,
    pub object_metadata_id: Uuid,
    #[serde(default)]
    pub is_label_synced_with_name: Option<bool>,
    #[serde(default)]
    pub is_remote_creation: Option<bool>,
    #[serde(default)]
    pub relation_creation_payload: Option<Json<Value>>,
    #[serde(default)]
    pub morph_relations_creation_payload: Option<Vec<Json<Value>>>,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateFieldInput")]
pub struct UpdateFieldMetadataInput {
    #[serde(default)]
    pub universal_identifier: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub is_system: Option<bool>,
    #[serde(default)]
    pub is_ui_editable: Option<bool>,
    #[serde(default)]
    pub is_ui_read_only: Option<bool>,
    #[serde(default)]
    pub is_nullable: Option<bool>,
    #[serde(default)]
    pub is_unique: Option<bool>,
    #[serde(default)]
    pub is_searchable: Option<bool>,
    #[serde(default)]
    pub is_audit_logged: Option<bool>,
    #[serde(default)]
    pub default_value: Option<Json<Value>>,
    #[serde(default)]
    pub options: Option<Json<Value>>,
    #[serde(default)]
    pub settings: Option<Json<Value>>,
    #[serde(default)]
    pub object_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub is_label_synced_with_name: Option<bool>,
    #[serde(default)]
    pub morph_relations_update_payload: Option<Vec<Json<Value>>>,
}

#[derive(Debug, Clone)]
pub struct RoleRecord {
    pub id: Uuid,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub is_editable: bool,
    pub can_update_all_settings: bool,
    pub can_access_all_tools: bool,
    pub can_read_all_object_records: bool,
    pub can_update_all_object_records: bool,
    pub can_soft_delete_all_object_records: bool,
    pub can_destroy_all_object_records: bool,
    pub can_be_assigned_to_users: bool,
    pub can_be_assigned_to_agents: bool,
    pub can_be_assigned_to_api_keys: bool,
    pub application_id: Uuid,
    pub universal_identifier: Option<String>,
}
