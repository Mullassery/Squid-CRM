//! Generic REST filter evaluation for the static Phase 5 object slices.
//!
//! The grammar mirrors the person filter parser: comma-separated leaves,
//! `and(...)`, `or(...)` and `not(...)`, with comparators such as `eq`, `in`,
//! `contains`, `startsWith`, `like`, `is` and range operators. Values are
//! coerced against the runtime type of the stored field.

use std::cmp::Ordering;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use squid_core::{AppError, AppResult};

use crate::sorting::SortValue;

#[derive(Debug, Clone)]
pub enum FilterNode {
    And(Vec<FilterNode>),
    Or(Vec<FilterNode>),
    Not(Box<FilterNode>),
    Leaf {
        field: String,
        operator: String,
        values: Vec<String>,
    },
}

pub fn parse_generic_filter(raw: &str, allowed: &[&str]) -> AppResult<FilterNode> {
    let mut parser = FilterParser::new(raw, allowed);
    let expr = parser.parse_expr()?;
    parser.skip_whitespace();
    if !parser.at_end() {
        return Err(AppError::BadRequest(format!(
            "Invalid filter near '{}'",
            parser.remaining()
        )));
    }
    Ok(expr)
}

pub fn references_prefix(node: &FilterNode, prefix: &str) -> bool {
    match node {
        FilterNode::And(items) | FilterNode::Or(items) => {
            items.iter().any(|item| references_prefix(item, prefix))
        }
        FilterNode::Not(item) => references_prefix(item, prefix),
        FilterNode::Leaf { field, .. } => field.starts_with(prefix),
    }
}

pub fn matches(node: &FilterNode, value: &impl Fn(&str) -> Option<SortValue>) -> bool {
    match node {
        FilterNode::And(items) => items.iter().all(|item| matches(item, value)),
        FilterNode::Or(items) => items.iter().any(|item| matches(item, value)),
        FilterNode::Not(item) => !matches(item, value),
        FilterNode::Leaf {
            field,
            operator,
            values,
        } => leaf_matches(value(field).as_ref(), operator, values),
    }
}

fn leaf_matches(actual: Option<&SortValue>, operator: &str, values: &[String]) -> bool {
    let first = values.first().map(String::as_str).unwrap_or_default();
    match operator {
        "is" => match first.to_ascii_uppercase().as_str() {
            "NULL" => actual.is_none(),
            "NOT_NULL" => actual.is_some(),
            _ => false,
        },
        "eq" => actual.is_some_and(|actual| equal(actual, first)),
        "neq" => !actual.is_some_and(|actual| equal(actual, first)),
        "in" => actual.is_some_and(|actual| values.iter().any(|value| equal(actual, value))),
        "contains" => text_value(actual).is_some_and(|text| text.contains(first)),
        "startsWith" => text_value(actual).is_some_and(|text| text.starts_with(first)),
        "endsWith" => text_value(actual).is_some_and(|text| text.ends_with(first)),
        "like" => text_value(actual).is_some_and(|text| like_match(first, text, false)),
        "ilike" => text_value(actual).is_some_and(|text| like_match(first, text, true)),
        "gt" | "gte" | "lt" | "lte" => actual.is_some_and(|actual| {
            let Some(expected) = coerce(actual, first) else {
                return false;
            };
            let ordering = compare(actual, &expected);
            match operator {
                "gt" => ordering == Ordering::Greater,
                "gte" => ordering != Ordering::Less,
                "lt" => ordering == Ordering::Less,
                _ => ordering != Ordering::Greater,
            }
        }),
        _ => false,
    }
}

fn text_value(value: Option<&SortValue>) -> Option<&str> {
    match value {
        Some(SortValue::Text(text)) => Some(text),
        _ => None,
    }
}

fn coerce(sample: &SortValue, raw: &str) -> Option<SortValue> {
    match sample {
        SortValue::Text(_) => Some(SortValue::Text(raw.to_string())),
        SortValue::Number(_) => raw.parse::<f64>().ok().map(SortValue::Number),
        SortValue::Bool(_) => match raw.to_ascii_lowercase().as_str() {
            "true" => Some(SortValue::Bool(true)),
            "false" => Some(SortValue::Bool(false)),
            _ => None,
        },
        SortValue::Time(_) => parse_time(raw).map(SortValue::Time),
    }
}

fn parse_time(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
        return Some(value.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|value| Utc.from_utc_datetime(&value))
}

fn equal(actual: &SortValue, raw: &str) -> bool {
    match actual {
        SortValue::Text(text) => text == raw,
        SortValue::Number(_) | SortValue::Bool(_) | SortValue::Time(_) => coerce(actual, raw)
            .is_some_and(|expected| compare(actual, &expected) == Ordering::Equal),
    }
}

fn compare(left: &SortValue, right: &SortValue) -> Ordering {
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

fn like_match(pattern: &str, text: &str, case_insensitive: bool) -> bool {
    let pattern: Vec<char> = if case_insensitive {
        pattern.to_lowercase().chars().collect()
    } else {
        pattern.chars().collect()
    };
    let text: Vec<char> = if case_insensitive {
        text.to_lowercase().chars().collect()
    } else {
        text.chars().collect()
    };
    like_chars(&pattern, &text)
}

fn like_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('%', rest)) => {
            like_chars(rest, text) || (!text.is_empty() && like_chars(pattern, &text[1..]))
        }
        Some(('_', rest)) => !text.is_empty() && like_chars(rest, &text[1..]),
        Some((expected, rest)) => text
            .split_first()
            .is_some_and(|(actual, tail)| actual == expected && like_chars(rest, tail)),
    }
}

struct FilterParser<'a> {
    chars: Vec<char>,
    pos: usize,
    allowed: &'a [&'a str],
}

impl<'a> FilterParser<'a> {
    fn new(input: &str, allowed: &'a [&'a str]) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            allowed,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let value = self.peek();
        if value.is_some() {
            self.pos += 1;
        }
        value
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn remaining(&self) -> String {
        self.chars[self.pos..].iter().collect()
    }

    fn expect(&mut self, expected: char) -> AppResult<()> {
        self.skip_whitespace();
        if self.bump() == Some(expected) {
            Ok(())
        } else {
            Err(AppError::BadRequest(format!(
                "Expected '{expected}' in filter near '{}'",
                self.remaining()
            )))
        }
    }

    fn read_ident(&mut self) -> String {
        self.skip_whitespace();
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn parse_expr(&mut self) -> AppResult<FilterNode> {
        self.skip_whitespace();
        let ident = self.read_ident();
        if ident == "or" && self.peek() == Some('(') {
            self.pos += 1;
            let items = self.parse_group()?;
            return Ok(FilterNode::Or(items));
        }
        if ident == "and" && self.peek() == Some('(') {
            self.pos += 1;
            let items = self.parse_group()?;
            return Ok(FilterNode::And(items));
        }
        if ident == "not" && self.peek() == Some('(') {
            self.pos += 1;
            let expr = self.parse_expr()?;
            self.expect(')')?;
            return Ok(FilterNode::Not(Box::new(expr)));
        }
        if ident.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Expected a field name in filter near '{}'",
                self.remaining()
            )));
        }
        if !self.allowed.contains(&ident.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Field \"{ident}\" does not exist or is not filterable"
            )));
        }

        self.expect('[')?;
        let start = self.pos;
        while self.peek().is_some_and(|c| c != ']') {
            self.pos += 1;
        }
        let operator: String = self.chars[start..self.pos].iter().collect();
        self.expect(']')?;
        self.expect(':')?;
        let values = self.read_values()?;

        Ok(FilterNode::Leaf {
            field: ident,
            operator: operator.trim().to_string(),
            values,
        })
    }

    fn parse_group(&mut self) -> AppResult<Vec<FilterNode>> {
        let mut items = Vec::new();
        loop {
            items.push(self.parse_expr()?);
            self.skip_whitespace();
            match self.bump() {
                Some(',') => continue,
                Some(')') => break,
                _ => return Err(AppError::BadRequest("Unbalanced filter group".to_string())),
            }
        }
        Ok(items)
    }

    fn read_values(&mut self) -> AppResult<Vec<String>> {
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some('(') || self.peek() == Some('[') {
            let close = if self.bump() == Some('(') { ')' } else { ']' };
            loop {
                values.push(self.read_value()?);
                self.skip_whitespace();
                match self.bump() {
                    Some(',') => continue,
                    Some(c) if c == close => break,
                    _ => {
                        return Err(AppError::BadRequest(
                            "Unbalanced value list in filter".to_string(),
                        ))
                    }
                }
            }
        } else {
            values.push(self.read_value()?);
        }
        Ok(values)
    }

    fn read_value(&mut self) -> AppResult<String> {
        self.skip_whitespace();
        match self.bump() {
            Some('"') | Some('\'') => {
                let quote = self.chars[self.pos - 1];
                let start = self.pos;
                while self.peek().is_some_and(|c| c != quote) {
                    self.pos += 1;
                }
                let value: String = self.chars[start..self.pos].iter().collect();
                self.expect(quote)?;
                Ok(value)
            }
            Some(first) => {
                let mut value = String::from(first);
                while self
                    .peek()
                    .is_some_and(|c| c != ',' && c != ')' && c != ']')
                {
                    value.push(self.chars[self.pos]);
                    self.pos += 1;
                }
                Ok(value.trim().to_string())
            }
            None => Err(AppError::BadRequest("Missing filter value".to_string())),
        }
    }
}
