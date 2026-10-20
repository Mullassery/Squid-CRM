//! Workspace view metadata consumed by the frontend `ViewFragment`.
//!
//! Only `viewFields` is seeded for the default person table. Filters, sorts,
//! groups and field groups are modelled so the query validates; they are empty.

use async_graphql::{Enum, InputObject, Json, SimpleObject};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

macro_rules! string_enum {
    ($name:ident, $gql:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Serialize, Deserialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
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

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewInput")]
pub struct CreateViewInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
    pub object_metadata_id: Uuid,
    #[serde(default)]
    pub view_type: Option<ViewType>,
    #[serde(default)]
    pub key: Option<ViewKey>,
    pub icon: String,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub is_compact: Option<bool>,
    #[serde(default)]
    pub should_hide_empty_groups: Option<bool>,
    #[serde(default)]
    pub kanban_column_width: Option<i32>,
    #[serde(default)]
    pub group_load_limit: Option<i32>,
    #[serde(default)]
    pub open_record_in: Option<ViewOpenRecordIn>,
    #[serde(default)]
    pub kanban_aggregate_operation: Option<AggregateOperations>,
    #[serde(default)]
    pub kanban_aggregate_operation_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub any_field_filter_value: Option<String>,
    #[serde(default)]
    pub calendar_layout: Option<ViewCalendarLayout>,
    #[serde(default)]
    pub calendar_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub calendar_end_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub visibility: Option<ViewVisibility>,
    #[serde(default)]
    pub main_group_by_field_metadata_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewInput")]
pub struct UpdateViewInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub view_type: Option<ViewType>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub is_compact: Option<bool>,
    #[serde(default)]
    pub open_record_in: Option<ViewOpenRecordIn>,
    #[serde(default)]
    pub kanban_aggregate_operation: Option<AggregateOperations>,
    #[serde(default)]
    pub kanban_aggregate_operation_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub any_field_filter_value: Option<String>,
    #[serde(default)]
    pub calendar_layout: Option<ViewCalendarLayout>,
    #[serde(default)]
    pub calendar_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub calendar_end_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub visibility: Option<ViewVisibility>,
    #[serde(default)]
    pub main_group_by_field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub should_hide_empty_groups: Option<bool>,
    #[serde(default)]
    pub kanban_column_width: Option<i32>,
    #[serde(default)]
    pub group_load_limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewFieldInput")]
pub struct CreateViewFieldInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub field_metadata_id: Uuid,
    pub view_id: Uuid,
    #[serde(default)]
    pub is_visible: Option<bool>,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub aggregate_operation: Option<AggregateOperations>,
    #[serde(default)]
    pub view_field_group_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewFieldInputUpdates")]
pub struct UpdateViewFieldInput {
    #[serde(default)]
    pub is_visible: Option<bool>,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub aggregate_operation: Option<AggregateOperations>,
    #[serde(default)]
    pub view_field_group_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewFilterInput")]
pub struct CreateViewFilterInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub field_metadata_id: Uuid,
    #[serde(default)]
    pub operand: Option<ViewFilterOperand>,
    pub value: Json<Value>,
    #[serde(default)]
    pub view_filter_group_id: Option<Uuid>,
    #[serde(default)]
    pub position_in_view_filter_group: Option<f64>,
    #[serde(default)]
    pub sub_field_name: Option<String>,
    #[serde(default)]
    pub relation_target_field_metadata_id: Option<Uuid>,
    pub view_id: Uuid,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewFilterInputUpdates")]
pub struct UpdateViewFilterInput {
    #[serde(default)]
    pub field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub operand: Option<ViewFilterOperand>,
    #[serde(default)]
    pub value: Option<Json<Value>>,
    #[serde(default)]
    pub view_filter_group_id: Option<Uuid>,
    #[serde(default)]
    pub position_in_view_filter_group: Option<f64>,
    #[serde(default)]
    pub sub_field_name: Option<String>,
    #[serde(default)]
    pub relation_target_field_metadata_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewSortInput")]
pub struct CreateViewSortInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub field_metadata_id: Uuid,
    #[serde(default)]
    pub direction: Option<ViewSortDirection>,
    #[serde(default)]
    pub sub_field_name: Option<String>,
    pub view_id: Uuid,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewSortInputUpdates")]
pub struct UpdateViewSortInput {
    #[serde(default)]
    pub direction: Option<ViewSortDirection>,
    #[serde(default)]
    pub sub_field_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewGroupInput")]
pub struct CreateViewGroupInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub is_visible: Option<bool>,
    pub field_value: String,
    #[serde(default)]
    pub position: Option<f64>,
    pub view_id: Uuid,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewGroupInputUpdates")]
pub struct UpdateViewGroupInput {
    #[serde(default)]
    pub field_metadata_id: Option<Uuid>,
    #[serde(default)]
    pub is_visible: Option<bool>,
    #[serde(default)]
    pub field_value: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewFieldGroupInput")]
pub struct CreateViewFieldGroupInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
    pub view_id: Uuid,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub is_visible: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewFieldGroupInputUpdates")]
pub struct UpdateViewFieldGroupInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub is_visible: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateViewFilterGroupInput")]
pub struct CreateViewFilterGroupInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub parent_view_filter_group_id: Option<Uuid>,
    #[serde(default)]
    pub logical_operator: Option<ViewFilterGroupLogicalOperator>,
    #[serde(default)]
    pub position_in_view_filter_group: Option<f64>,
    pub view_id: Uuid,
}

#[derive(Debug, Clone, Default, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateViewFilterGroupInput")]
pub struct UpdateViewFilterGroupInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub parent_view_filter_group_id: Option<Uuid>,
    #[serde(default)]
    pub logical_operator: Option<ViewFilterGroupLogicalOperator>,
    #[serde(default)]
    pub position_in_view_filter_group: Option<f64>,
    #[serde(default)]
    pub view_id: Option<Uuid>,
}
