//! Workspace view metadata consumed by the frontend `ViewFragment`.
//!
//! Only `viewFields` is seeded for the default person table. Filters, sorts,
//! groups and field groups are modelled so the query validates; they are empty.

use async_graphql::{Enum, Json, SimpleObject};
use serde_json::Value;
use uuid::Uuid;

macro_rules! string_enum {
    ($name:ident, $gql:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
        #[graphql(name = $gql)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(value: &str) -> Option<Self> {
                Some(match value {
                    $($text => Self::$variant),+,
                    _ => return None,
                })
            }
        }
    };
}

string_enum!(ViewType, "ViewType", {
    Table => "TABLE",
    Kanban => "KANBAN",
    Calendar => "CALENDAR",
    List => "LIST",
    FieldsWidget => "FIELDS_WIDGET",
    TableWidget => "TABLE_WIDGET",
    KanbanWidget => "KANBAN_WIDGET",
    ListWidget => "LIST_WIDGET",
    CalendarWidget => "CALENDAR_WIDGET",
});

string_enum!(AggregateOperations, "AggregateOperations", {
    Min => "MIN",
    Max => "MAX",
    Avg => "AVG",
    Sum => "SUM",
    Count => "COUNT",
    CountUniqueValues => "COUNT_UNIQUE_VALUES",
    CountEmpty => "COUNT_EMPTY",
    CountNotEmpty => "COUNT_NOT_EMPTY",
    CountTrue => "COUNT_TRUE",
    CountFalse => "COUNT_FALSE",
    PercentageEmpty => "PERCENTAGE_EMPTY",
    PercentageNotEmpty => "PERCENTAGE_NOT_EMPTY",
});

string_enum!(ViewFilterGroupLogicalOperator, "ViewFilterGroupLogicalOperator", {
    And => "AND",
    Or => "OR",
    Not => "NOT",
});

string_enum!(ViewFilterOperand, "ViewFilterOperand", {
    Is => "IS",
    IsNotNull => "IS_NOT_NULL",
    IsNot => "IS_NOT",
    LessThanOrEqual => "LESS_THAN_OR_EQUAL",
    GreaterThanOrEqual => "GREATER_THAN_OR_EQUAL",
    IsBefore => "IS_BEFORE",
    IsAfter => "IS_AFTER",
    Contains => "CONTAINS",
    DoesNotContain => "DOES_NOT_CONTAIN",
    IsEmpty => "IS_EMPTY",
    IsNotEmpty => "IS_NOT_EMPTY",
    IsRelative => "IS_RELATIVE",
    IsInPast => "IS_IN_PAST",
    IsInFuture => "IS_IN_FUTURE",
    IsToday => "IS_TODAY",
    VectorSearch => "VECTOR_SEARCH",
});

string_enum!(ViewSortDirection, "ViewSortDirection", {
    Asc => "ASC",
    Desc => "DESC",
});

string_enum!(ViewOpenRecordIn, "ViewOpenRecordIn", {
    SidePanel => "SIDE_PANEL",
    RecordPage => "RECORD_PAGE",
});

string_enum!(ViewCalendarLayout, "ViewCalendarLayout", {
    Day => "DAY",
    Week => "WEEK",
    Month => "MONTH",
});

string_enum!(ViewVisibility, "ViewVisibility", {
    Workspace => "WORKSPACE",
    Unlisted => "UNLISTED",
});

string_enum!(ViewKey, "ViewKey", {
    Index => "INDEX",
});

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewField {
    pub id: Uuid,
    pub universal_identifier: String,
    pub application_id: Uuid,
    pub is_system_side_effect: bool,
    pub field_metadata_id: Uuid,
    pub is_visible: bool,
    pub size: f64,
    pub position: f64,
    pub aggregate_operation: Option<AggregateOperations>,
    pub view_id: Uuid,
    pub view_field_group_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    #[graphql(name = "isOverridden")]
    pub is_overridden: Option<bool>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewFilterGroup {
    pub id: Uuid,
    pub parent_view_filter_group_id: Option<Uuid>,
    pub logical_operator: ViewFilterGroupLogicalOperator,
    pub position_in_view_filter_group: Option<f64>,
    pub view_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewFilter {
    pub id: Uuid,
    pub field_metadata_id: Uuid,
    pub operand: ViewFilterOperand,
    pub value: Json<Value>,
    pub view_filter_group_id: Option<Uuid>,
    pub position_in_view_filter_group: Option<f64>,
    pub sub_field_name: Option<String>,
    pub relation_target_field_metadata_id: Option<Uuid>,
    pub view_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewGroup {
    pub id: Uuid,
    pub is_visible: bool,
    pub field_value: String,
    pub position: f64,
    pub view_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewSort {
    pub id: Uuid,
    pub field_metadata_id: Uuid,
    pub direction: ViewSortDirection,
    pub sub_field_name: Option<String>,
    pub view_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ViewFieldGroup {
    pub id: Uuid,
    pub name: String,
    pub position: f64,
    pub is_visible: bool,
    pub view_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub view_fields: Vec<ViewField>,
    #[graphql(name = "isOverridden")]
    pub is_overridden: bool,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct View {
    pub id: Uuid,
    pub universal_identifier: String,
    pub application_id: Uuid,
    pub is_system_side_effect: bool,
    pub name: String,
    pub object_metadata_id: Uuid,
    #[graphql(name = "type")]
    pub view_type: ViewType,
    pub key: Option<ViewKey>,
    pub icon: String,
    pub position: f64,
    pub is_compact: bool,
    pub is_custom: bool,
    pub open_record_in: ViewOpenRecordIn,
    pub kanban_aggregate_operation: Option<AggregateOperations>,
    pub kanban_aggregate_operation_field_metadata_id: Option<Uuid>,
    pub main_group_by_field_metadata_id: Option<Uuid>,
    pub should_hide_empty_groups: bool,
    pub kanban_column_width: Option<i32>,
    pub group_load_limit: Option<i32>,
    pub calendar_field_metadata_id: Option<Uuid>,
    pub calendar_end_field_metadata_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub any_field_filter_value: Option<String>,
    pub calendar_layout: Option<ViewCalendarLayout>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub view_fields: Vec<ViewField>,
    pub view_filters: Vec<ViewFilter>,
    pub view_filter_groups: Vec<ViewFilterGroup>,
    pub view_sorts: Vec<ViewSort>,
    pub view_groups: Vec<ViewGroup>,
    pub view_field_groups: Vec<ViewFieldGroup>,
    pub visibility: ViewVisibility,
    pub created_by_user_workspace_id: Option<Uuid>,
    pub is_active: bool,
}
