//! Shared in-memory ordering for the static Phase 5 object slices.
//!
//! Cursor pagination is applied after ordering, so the order must be stable and
//! identical for GraphQL and REST. `SortValue` only covers the scalar columns
//! exposed by the current standard objects; relation ordering remains a
//! documented gap.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum SortValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Time(DateTime<Utc>),
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

fn compare_values(left: &SortValue, right: &SortValue) -> Ordering {
    match (left, right) {
        (SortValue::Text(left), SortValue::Text(right)) => left.cmp(right),
        (SortValue::Number(left), SortValue::Number(right)) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (SortValue::Bool(left), SortValue::Bool(right)) => left.cmp(right),
        (SortValue::Time(left), SortValue::Time(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

fn compare_term(left: Option<&SortValue>, right: Option<&SortValue>, term: &SortTerm) -> Ordering {
    match (left, right) {
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
            let ordering = compare_values(left, right);
            if term.descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
    }
}

pub fn sort_records<T>(
    items: &mut [T],
    terms: &[SortTerm],
    value: impl Fn(&T, &str) -> Option<SortValue>,
) {
    if terms.is_empty() {
        return;
    }
    items.sort_by(|left, right| {
        for term in terms {
            let left_value = value(left, &term.field);
            let right_value = value(right, &term.field);
            let ordering = compare_term(left_value.as_ref(), right_value.as_ref(), term);
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
}
