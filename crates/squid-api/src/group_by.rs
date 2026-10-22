//! REST `*/groupBy` support for the static standard-object slices.
//!
//! The captured API returns a bare array (no `data` envelope), aggregates as
//! strings, and maps empty group values to `null`. This module evaluates the
//! group-by request in memory over the same record set used by the list
//! endpoints; `viewId` filters are compiled into the shared generic filter AST.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use squid_core::{
    AppError, AppResult, View, ViewFilter, ViewFilterGroup, ViewFilterGroupLogicalOperator,
    ViewFilterOperand,
};
use uuid::Uuid;

use crate::filtering::{self, FilterNode};

#[derive(Debug, Clone)]
pub struct GroupByRequest {
    pub dimensions: Vec<GroupDimension>,
    pub aggregates: Vec<Aggregate>,
    pub include_records_sample: bool,
    pub order_by_for_records: Vec<SortTerm>,
    pub limit: usize,
    pub offset_for_records: usize,
}

#[derive(Debug, Clone)]
pub struct GroupDimension {
    pub field: String,
    pub sub_field: Option<String>,
    pub date: Option<DateGrouping>,
}

#[derive(Debug, Clone)]
pub struct DateGrouping {
    pub granularity: DateGranularity,
    pub time_zone: Option<Tz>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateGranularity {
    None,
    Day,
    Week,
    Month,
    Quarter,
    Year,
    DayOfTheWeek,
    MonthOfTheYear,
    QuarterOfTheYear,
}

#[derive(Debug, Clone)]
pub struct Aggregate {
    pub key: String,
    pub field: String,
    pub operation: AggregateOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateOperation {
    CountNotEmpty,
    CountEmpty,
    CountUniqueValues,
    PercentageEmpty,
    PercentageNotEmpty,
    CountTrue,
    CountFalse,
    Min,
    Max,
    Avg,
    Sum,
}

#[derive(Debug, Clone)]
pub struct SortTerm {
    pub field: String,
    pub descending: bool,
    pub nulls_first: bool,
}

impl SortTerm {
    pub fn parse(field: &str, direction: &str) -> Option<Self> {
        let (descending, nulls_first) = match direction {
            "AscNullsFirst" => (false, true),
            "AscNullsLast" => (false, false),
            "DescNullsFirst" => (true, true),
            "DescNullsLast" => (true, false),
            _ => return None,
        };
        Some(Self {
            field: field.to_string(),
            descending,
            nulls_first,
        })
    }
}

pub fn parse_request(
    group_by: Option<&str>,
    aggregate: Option<&str>,
    include_records_sample: Option<bool>,
    order_by_for_records: Option<&str>,
    limit: Option<i64>,
    offset_for_records: Option<i64>,
    allowed_fields: &[&str],
) -> AppResult<GroupByRequest> {
    let dimensions = parse_dimensions(group_by, allowed_fields)?;
    let aggregates = parse_aggregates(aggregate, allowed_fields)?;
    let order_by_for_records = parse_order_by(order_by_for_records, allowed_fields)?;
    let limit = match limit {
        Some(limit) if limit < 0 => {
            return Err(AppError::BadRequest(format!(
                "limit '{limit}' is invalid. Should be an integer"
            )))
        }
        Some(0) => usize::MAX,
        Some(limit) => limit.min(200) as usize,
        None => 50,
    };
    let offset_for_records = match offset_for_records {
        Some(offset) if offset < 0 => {
            return Err(AppError::BadRequest(format!(
                "offsetForRecords '{offset}' is invalid. Should be an integer"
            )))
        }
        Some(offset) => offset as usize,
        None => 0,
    };
    Ok(GroupByRequest {
        dimensions,
        aggregates,
        include_records_sample: include_records_sample.unwrap_or(false),
        order_by_for_records,
        limit,
        offset_for_records,
    })
}

fn parse_dimensions(raw: Option<&str>, allowed_fields: &[&str]) -> AppResult<Vec<GroupDimension>> {
    let Some(raw) = raw else {
        return Err(invalid_group_by());
    };
    let value: Value = serde_json::from_str(raw).map_err(|_| invalid_group_by())?;
    let Some(items) = value.as_array() else {
        return Err(invalid_group_by());
    };
    if items.is_empty() {
        return Err(invalid_group_by());
    }

    let allowed: HashSet<&str> = allowed_fields.iter().copied().collect();
    let mut dimensions = Vec::with_capacity(items.len());
    for item in items {
        let Some(object) = item.as_object() else {
            return Err(invalid_group_by());
        };
        if object.len() != 1 {
            return Err(invalid_group_by());
        }
        let (field, definition) = object.iter().next().expect("one entry");
        if field == "position" {
            return Err(AppError::BadRequest(
                "Field \"position\" is not supported in groupBy".to_string(),
            ));
        }

        match definition {
            Value::Bool(true) => {
                if !allowed.contains(field.as_str()) {
                    return Err(AppError::BadRequest(format!(
                        "Unidentified field in groupBy: {field}"
                    )));
                }
                dimensions.push(GroupDimension {
                    field: field.clone(),
                    sub_field: None,
                    date: None,
                });
            }
            Value::Object(definition) => {
                if definition.contains_key("granularity") {
                    if !allowed.contains(field.as_str()) {
                        return Err(AppError::BadRequest(format!(
                            "Unidentified field in groupBy: {field}"
                        )));
                    }
                    let granularity = definition
                        .get("granularity")
                        .and_then(Value::as_str)
                        .ok_or_else(invalid_group_by)
                        .and_then(parse_granularity)?;
                    let time_zone = definition
                        .get("timeZone")
                        .and_then(Value::as_str)
                        .map(|zone| {
                            zone.parse::<Tz>().map_err(|_| {
                                AppError::BadRequest(format!("Invalid timezone: {zone}"))
                            })
                        })
                        .transpose()?;
                    dimensions.push(GroupDimension {
                        field: field.clone(),
                        sub_field: None,
                        date: Some(DateGrouping {
                            granularity,
                            time_zone,
                        }),
                    });
                    continue;
                }

                let sub_fields: Vec<&String> = definition
                    .iter()
                    .filter(|(_, value)| value.as_bool() == Some(true))
                    .map(|(key, _)| key)
                    .collect();
                if sub_fields.len() != 1 {
                    return Err(AppError::BadRequest(
                        "You cannot provide multiple subfields in one GroupByInput, split them into multiple GroupByInput"
                            .to_string(),
                    ));
                }
                let sub_field = sub_fields[0].clone();
                let path = format!("{field}.{sub_field}");
                if !allowed.contains(path.as_str()) {
                    return Err(AppError::BadRequest(format!(
                        "Unidentified field in groupBy: {path}"
                    )));
                }
                dimensions.push(GroupDimension {
                    field: field.clone(),
                    sub_field: Some(sub_field),
                    date: None,
                });
            }
            _ => return Err(invalid_group_by()),
        }
    }
    Ok(dimensions)
}

fn parse_granularity(raw: &str) -> AppResult<DateGranularity> {
    Ok(match raw {
        "NONE" => DateGranularity::None,
        "DAY" => DateGranularity::Day,
        "WEEK" => DateGranularity::Week,
        "MONTH" => DateGranularity::Month,
        "QUARTER" => DateGranularity::Quarter,
        "YEAR" => DateGranularity::Year,
        "DAY_OF_THE_WEEK" => DateGranularity::DayOfTheWeek,
        "MONTH_OF_THE_YEAR" => DateGranularity::MonthOfTheYear,
        "QUARTER_OF_THE_YEAR" => DateGranularity::QuarterOfTheYear,
        other => {
            return Err(AppError::BadRequest(format!(
                "Invalid group by date granularity: {other}"
            )))
        }
    })
}

fn parse_aggregates(raw: Option<&str>, allowed_fields: &[&str]) -> AppResult<Vec<Aggregate>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| AppError::BadRequest("Invalid aggregate query parameter".to_string()))?;
    let Some(items) = value.as_array() else {
        return Err(AppError::BadRequest(
            "Invalid aggregate query parameter".to_string(),
        ));
    };
    let allowed: HashSet<&str> = allowed_fields.iter().copied().collect();
    let mut aggregates = Vec::with_capacity(items.len());
    let mut unknown = Vec::new();
    for item in items {
        let Some(key) = item.as_str() else {
            unknown.push(item.to_string());
            continue;
        };
        match parse_aggregate(key, &allowed) {
            Some(aggregate) => aggregates.push(aggregate),
            None => unknown.push(key.to_string()),
        }
    }
    if !unknown.is_empty() {
        return Err(AppError::BadRequest(format!(
            "Unknown aggregate field(s): {}",
            unknown.join(", ")
        )));
    }
    Ok(aggregates)
}

fn parse_aggregate(key: &str, allowed: &HashSet<&str>) -> Option<Aggregate> {
    const OPERATIONS: &[(&str, AggregateOperation)] = &[
        ("percentageNotEmpty", AggregateOperation::PercentageNotEmpty),
        ("percentageEmpty", AggregateOperation::PercentageEmpty),
        ("countUniqueValues", AggregateOperation::CountUniqueValues),
        ("countNotEmpty", AggregateOperation::CountNotEmpty),
        ("countEmpty", AggregateOperation::CountEmpty),
        ("countTrue", AggregateOperation::CountTrue),
        ("countFalse", AggregateOperation::CountFalse),
        ("min", AggregateOperation::Min),
        ("max", AggregateOperation::Max),
        ("avg", AggregateOperation::Avg),
        ("sum", AggregateOperation::Sum),
    ];
    for (prefix, operation) in OPERATIONS {
        if let Some(field) = key.strip_prefix(prefix) {
            if let Some(field) = resolve_field(field, allowed) {
                return Some(Aggregate {
                    key: key.to_string(),
                    field,
                    operation: *operation,
                });
            }
        }
    }
    None
}

fn resolve_field(field: &str, allowed: &HashSet<&str>) -> Option<String> {
    if allowed.contains(field) {
        return Some(field.to_string());
    }
    let mut chars = field.chars();
    let first = chars.next()?.to_ascii_lowercase();
    let candidate = format!("{first}{}", chars.as_str());
    allowed.contains(candidate.as_str()).then_some(candidate)
}

fn parse_order_by(raw: Option<&str>, allowed_fields: &[&str]) -> AppResult<Vec<SortTerm>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut terms = Vec::new();
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (field, direction) = match part.split_once('[') {
            Some((field, rest)) => {
                let direction = rest.strip_suffix(']').ok_or_else(|| {
                    AppError::BadRequest(
                        "'order_by' direction invalid. Allowed values are 'AscNullsFirst', \
                         'AscNullsLast', 'DescNullsFirst', 'DescNullsLast'."
                            .to_string(),
                    )
                })?;
                (field, direction)
            }
            None => (part, "AscNullsFirst"),
        };
        if !allowed_fields.contains(&field) {
            return Err(AppError::BadRequest(format!(
                "Field \"{field}\" does not exist or is not sortable"
            )));
        }
        let term = SortTerm::parse(field, direction).ok_or_else(|| {
            AppError::BadRequest(format!(
                "'order_by' direction '{direction}' invalid. Allowed values are \
                 'AscNullsFirst', 'AscNullsLast', 'DescNullsFirst', 'DescNullsLast'."
            ))
        })?;
        terms.push(term);
    }
    Ok(terms)
}

fn invalid_group_by() -> AppError {
    AppError::BadRequest(
        "Invalid group_by query parameter - should be a valid array of objects - ex: \
         [{\"firstField\": true}, {\"secondField\": {\"subField\": true}}, \
         {\"dateField\": {\"granularity\": 'DAY'}}]"
            .to_string(),
    )
}

/// Compile a stored view into the generic filter AST. Unknown/unfillable
/// filters are ignored because metadata-driven relation filters are not part of
/// the static object slice yet.
pub fn view_filter_node(view: &View, field_names: &HashMap<Uuid, String>) -> Option<FilterNode> {
    let mut filters_by_group: HashMap<Option<Uuid>, Vec<&ViewFilter>> = HashMap::new();
    for filter in &view.view_filters {
        filters_by_group
            .entry(filter.view_filter_group_id)
            .or_default()
            .push(filter);
    }
    let mut groups_by_parent: HashMap<Option<Uuid>, Vec<&ViewFilterGroup>> = HashMap::new();
    for group in &view.view_filter_groups {
        groups_by_parent
            .entry(group.parent_view_filter_group_id)
            .or_default()
            .push(group);
    }
    if filters_by_group.is_empty() && groups_by_parent.is_empty() {
        return None;
    }

    let mut nodes = Vec::new();
    for filter in filters_by_group.get(&None).into_iter().flatten() {
        if let Some(node) = filter_node(filter, field_names) {
            nodes.push(node);
        }
    }
    for group in groups_by_parent.get(&None).into_iter().flatten() {
        if let Some(node) = group_node(
            group.id,
            &filters_by_group,
            &groups_by_parent,
            field_names,
            &mut HashSet::new(),
        ) {
            nodes.push(node);
        }
    }
    match nodes.len() {
        0 => None,
        1 => nodes.pop(),
        _ => Some(FilterNode::And(nodes)),
    }
}

fn group_node(
    group_id: Uuid,
    filters_by_group: &HashMap<Option<Uuid>, Vec<&ViewFilter>>,
    groups_by_parent: &HashMap<Option<Uuid>, Vec<&ViewFilterGroup>>,
    field_names: &HashMap<Uuid, String>,
    seen: &mut HashSet<Uuid>,
) -> Option<FilterNode> {
    if !seen.insert(group_id) {
        return None;
    }
    let group = groups_by_parent
        .values()
        .flatten()
        .find(|group| group.id == group_id)?;
    let mut nodes = Vec::new();
    for filter in filters_by_group.get(&Some(group_id)).into_iter().flatten() {
        if let Some(node) = filter_node(filter, field_names) {
            nodes.push(node);
        }
    }
    for child in groups_by_parent.get(&Some(group_id)).into_iter().flatten() {
        if let Some(node) = group_node(
            child.id,
            filters_by_group,
            groups_by_parent,
            field_names,
            seen,
        ) {
            nodes.push(node);
        }
    }
    if nodes.is_empty() {
        return None;
    }
    Some(match group.logical_operator {
        ViewFilterGroupLogicalOperator::And => FilterNode::And(nodes),
        ViewFilterGroupLogicalOperator::Or => FilterNode::Or(nodes),
        ViewFilterGroupLogicalOperator::Not => FilterNode::Not(Box::new(FilterNode::And(nodes))),
    })
}

fn filter_node(filter: &ViewFilter, field_names: &HashMap<Uuid, String>) -> Option<FilterNode> {
    let mut field = field_names.get(&filter.field_metadata_id)?.clone();
    if let Some(sub_field) = &filter.sub_field_name {
        field = format!("{field}.{sub_field}");
    }
    let (operator, values) = view_operand(filter)?;
    Some(FilterNode::Leaf {
        field,
        operator: operator.to_string(),
        values,
    })
}

fn view_operand(filter: &ViewFilter) -> Option<(&'static str, Vec<String>)> {
    let value = &filter.value.0;
    let string_values = json_values(value);
    let first = string_values.first().cloned().unwrap_or_default();
    let (operator, values) = match filter.operand {
        ViewFilterOperand::Is => {
            if first.eq_ignore_ascii_case("NULL") || first.eq_ignore_ascii_case("NOT_NULL") {
                ("is", vec![first])
            } else {
                ("eq", string_values)
            }
        }
        ViewFilterOperand::IsNotNull => ("isNotEmpty", Vec::new()),
        ViewFilterOperand::IsNot => ("neq", string_values),
        ViewFilterOperand::Contains => ("contains", string_values),
        ViewFilterOperand::DoesNotContain => ("doesNotContain", string_values),
        ViewFilterOperand::IsEmpty => ("isEmpty", Vec::new()),
        ViewFilterOperand::IsNotEmpty => ("isNotEmpty", Vec::new()),
        ViewFilterOperand::LessThanOrEqual => ("lte", string_values),
        ViewFilterOperand::GreaterThanOrEqual => ("gte", string_values),
        ViewFilterOperand::IsBefore => ("lt", string_values),
        ViewFilterOperand::IsAfter => ("gt", string_values),
        ViewFilterOperand::IsInPast
        | ViewFilterOperand::IsInFuture
        | ViewFilterOperand::IsToday
        | ViewFilterOperand::IsRelative
        | ViewFilterOperand::VectorSearch => return None,
    };
    Some((operator, values))
}

fn json_values(value: &Value) -> Vec<String> {
    match value {
        Value::Null => vec!["NULL".to_string()],
        Value::String(value) => vec![value.clone()],
        Value::Bool(value) => vec![value.to_string()],
        Value::Number(value) => vec![value.to_string()],
        Value::Array(values) => values.iter().flat_map(json_values).collect(),
        Value::Object(_) => Vec::new(),
    }
}

pub fn matches(node: &FilterNode, record: &Value, date_fields: &HashSet<&str>) -> bool {
    filtering::matches(node, &|field| record_value(record, field, date_fields))
}

fn record_value(
    record: &Value,
    field: &str,
    date_fields: &HashSet<&str>,
) -> Option<crate::sorting::SortValue> {
    let value = path_value(record, field)?;
    if is_empty(value) {
        return None;
    }
    match value {
        Value::String(value) => {
            if date_fields.contains(field) {
                chrono::DateTime::parse_from_rfc3339(value)
                    .ok()
                    .map(|value| crate::sorting::SortValue::Time(value.with_timezone(&Utc)))
            } else {
                Some(crate::sorting::SortValue::Text(value.clone()))
            }
        }
        Value::Number(value) => value.as_f64().map(crate::sorting::SortValue::Number),
        Value::Bool(value) => Some(crate::sorting::SortValue::Bool(*value)),
        _ => None,
    }
}

fn path_value<'a>(record: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = record;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.is_empty(),
        Value::Array(values) => values.is_empty() || values.iter().all(is_empty),
        Value::Object(values) => values.is_empty() || values.values().all(is_empty),
        _ => false,
    }
}

pub fn execute(
    records: &[Value],
    request: &GroupByRequest,
    date_fields: &HashSet<&str>,
) -> AppResult<Vec<Value>> {
    let mut groups: BTreeMap<Vec<GroupKey>, Vec<&Value>> = BTreeMap::new();
    for record in records {
        let mut key = Vec::with_capacity(request.dimensions.len());
        for dimension in &request.dimensions {
            key.push(group_key(record, dimension)?);
        }
        groups.entry(key).or_default().push(record);
    }

    let mut output = Vec::new();
    for (key, mut members) in groups {
        if request.include_records_sample {
            sort_records(&mut members, &request.order_by_for_records, date_fields);
            members = members
                .into_iter()
                .skip(request.offset_for_records)
                .collect();
        }
        let mut group = serde_json::Map::new();
        for aggregate in &request.aggregates {
            group.insert(
                aggregate.key.clone(),
                Value::String(aggregate_value(aggregate, &members, date_fields)),
            );
        }
        group.insert(
            "groupByDimensionValues".to_string(),
            Value::Array(key.into_iter().map(GroupKey::into_json).collect()),
        );
        if request.include_records_sample {
            group.insert(
                "records".to_string(),
                Value::Array(members.into_iter().cloned().collect()),
            );
        }
        output.push(Value::Object(group));
    }
    output.truncate(request.limit);
    Ok(output)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Text(String),
    Null,
}

impl GroupKey {
    fn into_json(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Text(value) => Value::String(value),
        }
    }
}

fn group_key(record: &Value, dimension: &GroupDimension) -> AppResult<GroupKey> {
    let path = match &dimension.sub_field {
        Some(sub_field) => format!("{}.{}", dimension.field, sub_field),
        None => dimension.field.clone(),
    };
    let value = path_value(record, &path);
    if let Some(date) = &dimension.date {
        let Some(Value::String(raw)) = value else {
            return Ok(GroupKey::Null);
        };
        let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(raw) else {
            return Ok(GroupKey::Null);
        };
        return Ok(GroupKey::Text(format_date(
            timestamp.with_timezone(&Utc),
            date,
        )));
    }
    match value {
        None => Ok(GroupKey::Null),
        Some(value) if is_empty(value) => Ok(GroupKey::Null),
        Some(Value::String(value)) => Ok(GroupKey::Text(value.clone())),
        Some(Value::Bool(value)) => Ok(GroupKey::Text(value.to_string())),
        Some(Value::Number(value)) => Ok(GroupKey::Text(value.to_string())),
        Some(_) => Ok(GroupKey::Null),
    }
}

fn format_date(value: chrono::DateTime<Utc>, grouping: &DateGrouping) -> String {
    let zone = grouping.time_zone.unwrap_or(Tz::UTC);
    let local = value.with_timezone(&zone);
    let date = local.date_naive();
    match grouping.granularity {
        DateGranularity::None => value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        DateGranularity::Day => date.to_string(),
        DateGranularity::Week => {
            let start = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
            start.to_string()
        }
        DateGranularity::Month => format!("{:04}-{:02}-01", date.year(), date.month()),
        DateGranularity::Quarter => {
            let month = ((date.month() - 1) / 3) * 3 + 1;
            format!("{:04}-{:02}-01", date.year(), month)
        }
        DateGranularity::Year => format!("{:04}-01-01", date.year()),
        DateGranularity::DayOfTheWeek => date.weekday().to_string(),
        DateGranularity::MonthOfTheYear => date.format("%B").to_string(),
        DateGranularity::QuarterOfTheYear => format!("Q{}", (date.month() - 1) / 3 + 1),
    }
}

fn aggregate_value(
    aggregate: &Aggregate,
    records: &[&Value],
    date_fields: &HashSet<&str>,
) -> String {
    let values: Vec<&Value> = records
        .iter()
        .filter_map(|record| path_value(record, &aggregate.field))
        .collect();
    let non_empty: Vec<&Value> = values
        .iter()
        .copied()
        .filter(|value| !is_empty(value))
        .collect();
    let total = records.len();
    let empty = total.saturating_sub(non_empty.len());
    match aggregate.operation {
        AggregateOperation::CountNotEmpty => non_empty.len().to_string(),
        AggregateOperation::CountEmpty => empty.to_string(),
        AggregateOperation::CountUniqueValues => {
            let unique: HashSet<String> =
                non_empty.iter().map(|value| scalar_text(value)).collect();
            unique.len().to_string()
        }
        AggregateOperation::PercentageEmpty => ratio(empty, total),
        AggregateOperation::PercentageNotEmpty => ratio(non_empty.len(), total),
        AggregateOperation::CountTrue => non_empty
            .iter()
            .filter(|value| value.as_bool() == Some(true))
            .count()
            .to_string(),
        AggregateOperation::CountFalse => non_empty
            .iter()
            .filter(|value| value.as_bool() == Some(false))
            .count()
            .to_string(),
        AggregateOperation::Min => extreme(&non_empty, date_fields, true),
        AggregateOperation::Max => extreme(&non_empty, date_fields, false),
        AggregateOperation::Avg => {
            let numbers = numeric_values(&non_empty);
            if numbers.is_empty() {
                return String::new();
            }
            (numbers.iter().sum::<f64>() / numbers.len() as f64).to_string()
        }
        AggregateOperation::Sum => {
            let numbers = numeric_values(&non_empty);
            numbers.iter().sum::<f64>().to_string()
        }
    }
}

fn numeric_values(values: &[&Value]) -> Vec<f64> {
    values
        .iter()
        .filter_map(|value| match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.parse::<f64>().ok(),
            _ => None,
        })
        .collect()
}

fn extreme(values: &[&Value], date_fields: &HashSet<&str>, min: bool) -> String {
    let mut best: Option<&Value> = None;
    for value in values.iter().copied() {
        let Some(order) = best.map(|current| compare_json(current, value, date_fields)) else {
            best = Some(value);
            continue;
        };
        if (min && order == Ordering::Greater) || (!min && order == Ordering::Less) {
            best = Some(value);
        }
    }
    best.map(scalar_text).unwrap_or_default()
}

fn ratio(part: usize, total: usize) -> String {
    if total == 0 {
        return "0.00000000000000000000".to_string();
    }
    format!("{:.20}", part as f64 / total as f64)
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(values) => values.iter().map(scalar_text).collect::<Vec<_>>().join(","),
        Value::Object(_) => String::new(),
    }
}

fn sort_records(records: &mut [&Value], terms: &[SortTerm], date_fields: &HashSet<&str>) {
    if terms.is_empty() {
        return;
    }
    records.sort_by(|left, right| {
        for term in terms {
            let left = path_value(left, &term.field);
            let right = path_value(right, &term.field);
            let ordering = match (left, right) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => {
                    if term.nulls_first {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (Some(_), None) => {
                    if term.nulls_first {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
                (Some(left), Some(right)) => {
                    let ordering = compare_json(left, right, date_fields);
                    if term.descending {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                }
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
}

fn compare_json(left: &Value, right: &Value, date_fields: &HashSet<&str>) -> Ordering {
    let _ = date_fields;
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .partial_cmp(&right.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => scalar_text(left).cmp(&scalar_text(right)),
    }
}

pub fn date_field_set() -> HashSet<&'static str> {
    ["createdAt", "updatedAt", "deletedAt", "dueAt", "closeDate"]
        .into_iter()
        .collect()
}

pub fn json_record<T: serde::Serialize>(record: &T) -> AppResult<Value> {
    serde_json::to_value(record).map_err(|error| AppError::Internal(anyhow::anyhow!(error)))
}

/// Test/helper: parse a JSON date value into the local date used by groupBy.
pub fn parse_date(
    raw: &str,
    granularity: DateGranularity,
    timezone: Option<&str>,
) -> Option<String> {
    let value = chrono::DateTime::parse_from_rfc3339(raw)
        .ok()?
        .with_timezone(&Utc);
    let grouping = DateGrouping {
        granularity,
        time_zone: timezone.and_then(|zone| zone.parse::<Tz>().ok()),
    };
    Some(format_date(value, &grouping))
}

// Keep NaiveDate/TimeZone imports used on older chrono versions where
// `date_naive` is gated; these helpers also keep the module explicit.
#[allow(dead_code)]
fn local_date(value: chrono::DateTime<Utc>, zone: Tz) -> NaiveDate {
    zone.from_utc_datetime(&value.naive_utc()).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<&'static str> {
        vec![
            "id",
            "createdAt",
            "jobTitle",
            "name.firstName",
            "emails.primaryEmail",
        ]
    }

    #[test]
    fn groups_empty_strings_as_null_and_counts() {
        let records = vec![
            json!({"id": "1", "jobTitle": "Engineer", "createdAt": "2026-01-01T00:00:00Z"}),
            json!({"id": "2", "jobTitle": "", "createdAt": "2026-01-02T00:00:00Z"}),
        ];
        let request = parse_request(
            Some(r#"[{"jobTitle":true}]"#),
            Some(r#"["countNotEmptyId","countEmptyJobTitle"]"#),
            Some(true),
            Some("createdAt[DescNullsLast]"),
            None,
            None,
            &fields(),
        )
        .unwrap();
        let output = execute(&records, &request, &date_field_set()).unwrap();
        assert_eq!(output[0]["groupByDimensionValues"], json!(["Engineer"]));
        assert_eq!(output[1]["groupByDimensionValues"], json!([null]));
        assert_eq!(output[1]["countNotEmptyId"], "1");
        assert_eq!(output[1]["countEmptyJobTitle"], "1");
    }

    #[test]
    fn validates_unknown_field_and_aggregate() {
        assert!(parse_request(
            Some(r#"[{"nope":true}]"#),
            None,
            None,
            None,
            None,
            None,
            &fields(),
        )
        .is_err());
        assert!(parse_request(
            Some(r#"[{"jobTitle":true}]"#),
            Some(r#"["wat"]"#),
            None,
            None,
            None,
            None,
            &fields(),
        )
        .is_err());
    }

    #[test]
    fn truncates_dates_with_timezone() {
        assert_eq!(
            parse_date(
                "2026-10-01T03:00:00Z",
                DateGranularity::Month,
                Some("America/New_York")
            )
            .unwrap(),
            "2026-09-01"
        );
    }
}
