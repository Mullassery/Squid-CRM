//! Shared in-memory ordering for the static Phase 5 object slices.
//!
//! Cursor pagination is applied after ordering, so the order must be stable and
//! identical for GraphQL and REST. `SortValue` only covers the scalar columns
//! exposed by the current standard objects; relation ordering remains a
//! documented gap.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};
use squid_core::{Company, Note, NoteTarget, Opportunity, Person, Task, TaskTarget};

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

fn text(value: impl ToString) -> SortValue {
    SortValue::Text(value.to_string())
}

pub fn company_value(company: &Company, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(company.id)),
        "createdAt" => Some(SortValue::Time(company.created_at)),
        "updatedAt" => Some(SortValue::Time(company.updated_at)),
        "deletedAt" => company.deleted_at.map(SortValue::Time),
        "name" => company.name.clone().map(SortValue::Text),
        "position" => Some(SortValue::Number(company.position)),
        "accountOwnerId" => company.account_owner_id.map(text),
        "searchVector" => company.search_vector.clone().map(SortValue::Text),
        "annualRevenue" | "annualRevenue.amountMicros" => company
            .annual_revenue
            .amount_micros
            .map(|value| SortValue::Number(value as f64)),
        _ => None,
    }
}

pub fn person_value(person: &Person, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(person.id)),
        "createdAt" => Some(SortValue::Time(person.created_at)),
        "updatedAt" => Some(SortValue::Time(person.updated_at)),
        "deletedAt" => person.deleted_at.map(SortValue::Time),
        "name.firstName" => Some(SortValue::Text(person.name.first_name.clone())),
        "name.lastName" => Some(SortValue::Text(person.name.last_name.clone())),
        "emails.primaryEmail" => Some(SortValue::Text(person.emails.primary_email.clone())),
        "jobTitle" => person.job_title.clone().map(SortValue::Text),
        "companyId" => person.company_id.map(text),
        "position" => Some(SortValue::Number(person.position)),
        "searchVector" => person.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}

pub fn opportunity_value(opportunity: &Opportunity, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(opportunity.id)),
        "createdAt" => Some(SortValue::Time(opportunity.created_at)),
        "updatedAt" => Some(SortValue::Time(opportunity.updated_at)),
        "deletedAt" => opportunity.deleted_at.map(SortValue::Time),
        "name" => opportunity.name.clone().map(SortValue::Text),
        "amount" | "amount.amountMicros" => opportunity
            .amount
            .amount_micros
            .map(|value| SortValue::Number(value as f64)),
        "closeDate" => opportunity.close_date.map(SortValue::Time),
        "stage" => Some(text(opportunity.stage.as_str())),
        "position" => Some(SortValue::Number(opportunity.position)),
        "companyId" => opportunity.company_id.map(text),
        "pointOfContactId" => opportunity.point_of_contact_id.map(text),
        "ownerId" => opportunity.owner_id.map(text),
        "searchVector" => opportunity.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}

pub fn task_value(task: &Task, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(task.id)),
        "createdAt" => Some(SortValue::Time(task.created_at)),
        "updatedAt" => Some(SortValue::Time(task.updated_at)),
        "deletedAt" => task.deleted_at.map(SortValue::Time),
        "position" => Some(SortValue::Number(task.position)),
        "title" => task.title.clone().map(SortValue::Text),
        "dueAt" => task.due_at.map(SortValue::Time),
        "status" => task.status.map(|status| text(status.as_str())),
        "assigneeId" => task.assignee_id.map(text),
        "searchVector" => task.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}

pub fn note_value(note: &Note, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(note.id)),
        "createdAt" => Some(SortValue::Time(note.created_at)),
        "updatedAt" => Some(SortValue::Time(note.updated_at)),
        "deletedAt" => note.deleted_at.map(SortValue::Time),
        "position" => Some(SortValue::Number(note.position)),
        "title" => note.title.clone().map(SortValue::Text),
        "searchVector" => note.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}

pub fn task_target_value(target: &TaskTarget, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(target.id)),
        "createdAt" => Some(SortValue::Time(target.created_at)),
        "updatedAt" => Some(SortValue::Time(target.updated_at)),
        "deletedAt" => target.deleted_at.map(SortValue::Time),
        "position" => Some(SortValue::Number(target.position)),
        "taskId" => Some(text(target.task_id)),
        "targetCompanyId" => target.target_company_id.map(text),
        "targetOpportunityId" => target.target_opportunity_id.map(text),
        "targetPersonId" => target.target_person_id.map(text),
        "searchVector" => target.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}

pub fn note_target_value(target: &NoteTarget, field: &str) -> Option<SortValue> {
    match field {
        "id" => Some(text(target.id)),
        "createdAt" => Some(SortValue::Time(target.created_at)),
        "updatedAt" => Some(SortValue::Time(target.updated_at)),
        "deletedAt" => target.deleted_at.map(SortValue::Time),
        "position" => Some(SortValue::Number(target.position)),
        "noteId" => Some(text(target.note_id)),
        "targetCompanyId" => target.target_company_id.map(text),
        "targetOpportunityId" => target.target_opportunity_id.map(text),
        "targetPersonId" => target.target_person_id.map(text),
        "searchVector" => target.search_vector.clone().map(SortValue::Text),
        _ => None,
    }
}
