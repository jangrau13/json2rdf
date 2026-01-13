// Copyright (c) 2024-2025, DeciSym, LLC
// Licensed under either of:
// - Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// - BSD 3-Clause License (https://opensource.org/licenses/BSD-3-Clause)
// at your option.

//! # JSON2RDF Converter Library
//!
//! This library provides functionality for converting JSON data into RDF format.
//! It uses `serde_json` for JSON parsing and `oxrdf` to build and manage RDF graphs.
//!
//! ## Overview
//! - Converts JSON data structures into RDF triples, generating a graph representation.
//! - Supports blank nodes for nested structures and maps JSON properties to RDF predicates.
//! - Uses blake3 hashing for root node identification instead of blank nodes.
//!
//! ## Features
//! - Handles JSON Objects, Arrays, Booleans, Numbers, and Strings as RDF triples.
//! - Allows specifying a custom RDF namespace for generated predicates and objects.
//! - Generates a blake3-hashed named node for the root JSON object.
//! - Outputs the RDF data to a specified file or prints it to the console.

use oxrdf::vocab::xsd;
use oxrdf::{BlankNode, Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, TripleRef};

use serde_json::{Deserializer, Value};
use std::collections::VecDeque;
use std::io::{self, Read};
use urlencoding;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub mod writer;

use crate::writer::RdfWriter;

/// Compute a stable hash of a string that doesn't depend on RNG
/// This replaces blake3::hash which fails in WASM with thread-local RNG issues
fn compute_stable_hash(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash_value = hasher.finish();
    format!("{:x}", hash_value)
}

/// Normalizes a namespace URI to have a consistent format.
/// Ensures it ends with either '#' or '/' and is a valid URI.
fn normalize_namespace(ns: &str) -> String {
    let trimmed = ns.trim();
    if trimmed.is_empty() {
        return "https://purl.org/wiser/json2rdf/model#".to_string();
    }

    // Remove trailing slashes and hashes
    let base = trimmed.trim_end_matches('/').trim_end_matches('#');

    // Prefer '#' as the separator for RDF properties
    format!("{}#", base)
}

/// Creates a property URI with proper encoding and namespace formatting.
fn create_property_uri(namespace: &str, property_name: &str) -> String {
    let normalized_ns = normalize_namespace(namespace);
    // Remove the trailing '#' temporarily, we'll add the encoded property name after
    let base_ns = normalized_ns.trim_end_matches('#');
    format!("{}#{}", base_ns, urlencoding::encode(property_name))
}

/// Subject enumeration to support both NamedNode and BlankNode as identifiers
#[derive(Clone)]
enum SubjectNode {
    Named(String),
    Blank(BlankNode),
}

impl SubjectNode {
    /// Convert to NamedOrBlankNode for use in RDF triples
    fn to_named_or_blank_node(&self) -> NamedOrBlankNode {
        match self {
            SubjectNode::Named(uri) => {
                NamedOrBlankNode::NamedNode(NamedNode::new(uri.clone()).unwrap())
            }
            SubjectNode::Blank(bn) => NamedOrBlankNode::BlankNode(bn.clone()),
        }
    }
}

/// Converts JSON data to RDF format.
///
/// This function reads JSON data from the provided reader, processes it into RDF triples,
/// and outputs the RDF data using the provided writer. Users can specify a namespace to use for RDF predicates.
/// The root JSON object is assigned a NamedNode identified by the blake3 hash of its content.
///
/// # Arguments
/// - `reader`: Reader providing JSON data (in-memory or file-based).
/// - `writer`: Writer to output the RDF triples.
/// - `namespace`: Optional custom namespace for RDF predicates.
///
/// # Example
/// ```rust
/// use json2rdf::json_to_rdf;
/// use json2rdf::writer;
/// use oxrdf::Graph;
/// use std::fs::File;
/// use std::io::Read;
///
/// fn run() -> Result<(), Box<dyn std::error::Error>> {
///     // Use an in-memory JSON string and a GraphWriter for the example so the doctest
///     // doesn't depend on an external file existing during test runs.
///     use std::io::Cursor;
///     let json_data = r#"{"key": "value"}"#;
///     let mut g = Graph::new();
///     let mut w = writer::GraphWriter::new(&mut g);
///     json_to_rdf(Cursor::new(json_data.as_bytes()), &mut w, &Some("http://example.com/ns#".to_string()))?;
///     Ok(())
/// }
///
/// run().unwrap();
/// ```
pub fn json_to_rdf<R: Read>(
    reader: R,
    writer: &mut dyn RdfWriter,
    namespace: &Option<String>,
) -> Result<(), io::Error> {
    let default_namespace = "https://purl.org/wiser/json2rdf/model".to_owned();
    let rdf_namespace: String = normalize_namespace(
        namespace.as_ref().unwrap_or(&default_namespace)
    );

    let buf_reader = std::io::BufReader::new(reader);
    let stream = Deserializer::from_reader(buf_reader).into_iter::<Value>();

    let mut subject_stack: VecDeque<SubjectNode> = VecDeque::new();
    let mut property: Option<String> = None;
    let mut is_root = true;
    let mut blank_node_counter: u64 = 0;

    for value in stream {
        match value {
            Ok(Value::Object(obj)) => {
                let subject = if is_root {
                    is_root = false;
                    // For the root node, create a NamedNode using stable hash (no RNG dependency)
                    let json_str = serde_json::to_string(&obj)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    let hash_hex = compute_stable_hash(&json_str);
                    let root_uri = format!(
                        "{}#{}",
                        rdf_namespace.trim_end_matches('#'),
                        hash_hex
                    );
                    SubjectNode::Named(root_uri)
                } else {
                    // Create deterministic blank node ID using counter (no RNG)
                    let bn_id = format!("b{}", blank_node_counter);
                    blank_node_counter += 1;
                    SubjectNode::Blank(BlankNode::new(bn_id).unwrap())
                };
                subject_stack.push_back(subject);

                for (key, val) in obj {
                    property = Some(create_property_uri(&rdf_namespace, &key));
                    process_value(
                        &mut subject_stack,
                        &property,
                        val,
                        writer,
                        &rdf_namespace,
                        &mut blank_node_counter,
                    );
                }

                subject_stack.pop_back();
            }
            Ok(Value::Array(arr)) => {
                for val in arr {
                    process_value(
                        &mut subject_stack,
                        &property,
                        val,
                        writer,
                        &rdf_namespace.clone(),
                        &mut blank_node_counter,
                    );
                }
            }
            Ok(other) => {
                process_value(
                    &mut subject_stack,
                    &property,
                    other,
                    writer,
                    &rdf_namespace.clone(),
                    &mut blank_node_counter,
                );
            }
            Err(e) => {
                eprintln!("Error parsing JSON: {}", e);
            }
        }
    }

    Ok(())
}

/// This function handles different JSON data types, converting each into RDF triples:
/// - JSON Objects create new blank nodes and recursively process nested values.
/// - JSON Arrays iterate over each element and process it as an individual value.
/// - JSON Booleans, Numbers, and Strings are converted to RDF literals.
///
/// # Recursion for Nested Structures
/// Recursion is used to handle deeply nested JSON structures, which may contain multiple
/// levels of objects or arrays. This recursive approach allows the function to "dive" into
/// each nested layer of a JSON structure, creating blank nodes for sub-objects and handling
/// them as new subjects within the RDF graph. As a result, each level of JSON data is
/// systematically transformed into RDF triples, regardless of complexity or depth.
///
/// # Arguments
/// - `subject_stack`: Stack of subjects (NamedNode or BlankNode). Each nested level pushes a new subject to the stack.
/// - `property`: RDF predicate (property) associated with the JSON value.
/// - `value`: JSON value to process.
/// - `writer`: Writer to output RDF triples.
/// - `namespace`: Namespace for generating predicate URIs.
///
/// # JSON Type to RDF Conversion
/// - **Object**: Creates a blank node and recursively processes key-value pairs.
/// - **Array**: Iterates over elements and processes each as a separate value.
/// - **String**: Converts to `xsd:string` literal.
/// - **Boolean**: Converts to `xsd:boolean` literal.
/// - **Number**: Converts to `xsd:int` or `xsd:float` literal based on value type.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_namespace_with_trailing_slash() {
        let result = normalize_namespace("https://example.com/");
        assert_eq!(result, "https://example.com#");
    }

    #[test]
    fn test_normalize_namespace_with_trailing_hash() {
        let result = normalize_namespace("https://example.com#");
        assert_eq!(result, "https://example.com#");
    }

    #[test]
    fn test_normalize_namespace_without_trailing() {
        let result = normalize_namespace("https://example.com");
        assert_eq!(result, "https://example.com#");
    }

    #[test]
    fn test_normalize_namespace_empty() {
        let result = normalize_namespace("");
        assert_eq!(result, "https://purl.org/wiser/json2rdf/model#");
    }

    #[test]
    fn test_create_property_uri_simple() {
        let result = create_property_uri("https://example.com#", "propertyName");
        assert_eq!(result, "https://example.com#propertyName");
    }

    #[test]
    fn test_create_property_uri_with_special_chars() {
        let result = create_property_uri("https://example.com", "property Name");
        assert_eq!(result, "https://example.com#property%20Name");
    }

    #[test]
    fn test_create_property_uri_with_trailing_slash() {
        let result = create_property_uri("https://example.com/", "propertyName");
        assert_eq!(result, "https://example.com#propertyName");
    }
}

fn process_value(
    subject_stack: &mut VecDeque<SubjectNode>,
    property: &Option<String>,
    value: Value,
    writer: &mut dyn RdfWriter,
    namespace: &String,
    blank_node_counter: &mut u64,
) {
    // Normalize namespace to ensure consistent formatting with '#' separator
    let normalized_ns = normalize_namespace(namespace);

    if let Some(last_subject) = subject_stack.back().cloned() {
        if let Some(prop) = property {
            match value {
                Value::Bool(b) => {
                    let literal = Literal::new_typed_literal(b.to_string(), xsd::BOOLEAN);
                    let subject_nob = last_subject.to_named_or_blank_node();
                    let triple = TripleRef::new(
                        &subject_nob,
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &literal,
                    );
                    writer.add_triple(triple).unwrap();
                }
                Value::Number(num) => {
                    let literal = if num.as_i64().is_some() {
                        Literal::new_typed_literal(num.to_string(), xsd::INT)
                    } else if num.as_f64().is_some() {
                        Literal::new_typed_literal(num.to_string(), xsd::FLOAT)
                    } else {
                        return;
                    };
                    let subject_nob = last_subject.to_named_or_blank_node();
                    let triple = TripleRef::new(
                        &subject_nob,
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &literal,
                    );
                    writer.add_triple(triple).unwrap();
                }
                Value::String(s) => {
                    let literal = Literal::new_typed_literal(s, xsd::STRING);
                    let subject_nob = last_subject.to_named_or_blank_node();
                    let triple = TripleRef::new(
                        &subject_nob,
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &literal,
                    );
                    writer.add_triple(triple).unwrap();
                }
                Value::Null => {
                    //println!("Null value");
                }
                Value::Object(obj) => {
                    // Create deterministic blank node ID using counter (no RNG)
                    let bn_id = format!("b{}", blank_node_counter);
                    *blank_node_counter += 1;
                    let subject = SubjectNode::Blank(BlankNode::new(bn_id).unwrap());
                    subject_stack.push_back(subject.clone());

                    let last_subject_nob = last_subject.to_named_or_blank_node();
                    let new_subject_nob = subject.to_named_or_blank_node();
                    let triple = TripleRef::new(
                        &last_subject_nob,
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &new_subject_nob,
                    );
                    writer.add_triple(triple).unwrap();

                    for (key, val) in obj {
                        let nested_property: Option<String> = Some(create_property_uri(&normalized_ns, &key));
                        process_value(subject_stack, &nested_property, val, writer, &normalized_ns, blank_node_counter);
                    }
                    subject_stack.pop_back();
                }
                Value::Array(arr) => {
                    for val in arr {
                        process_value(subject_stack, property, val, writer, &normalized_ns, blank_node_counter);
                    }
                }
            }
        }
    }
}
