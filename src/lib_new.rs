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

use blake3;
use clap::Error;
use oxrdf::vocab::xsd;
use oxrdf::{BlankNode, Literal, NamedNode, NamedNodeRef, Subject, TripleRef};

use serde_json::{Deserializer, Value};
use std::collections::VecDeque;
use std::io::Read;
use urlencoding;

pub mod writer;

use crate::writer::RdfWriter;

/// Subject enumeration to support both NamedNode and BlankNode
enum SubjectNode {
    Named(NamedNode),
    Blank(BlankNode),
}

impl SubjectNode {
    fn as_ref(&self) -> Subject {
        match self {
            SubjectNode::Named(n) => Subject::NamedNode(n.clone()),
            SubjectNode::Blank(b) => Subject::BlankNode(b.clone()),
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
/// // From a file:
/// let file = File::open("data.json")?;
/// let mut w = writer::FileWriter::to_vec();
/// json_to_rdf(Box::new(file), &mut w, &Some("http://example.com/ns#".to_string()));
///
/// // From an in-memory string:
/// let json_data = r#"{"key": "value"}"#.as_bytes();
/// let mut g = Graph::new();
/// let mut w = writer::GraphWriter::new(&mut g);
/// json_to_rdf(Box::new(json_data), &mut w, &Some("http://example.com/ns#".to_string()));
/// ```
pub fn json_to_rdf<R: Read>(
    reader: R,
    writer: &mut dyn RdfWriter,
    namespace: &Option<String>,
) -> Result<(), Error> {
    let default_namespace = "https://purl.org/wiser/json2rdf/model".to_owned();
    let rdf_namespace: String = if let Some(ns) = namespace {
        ns.clone()
    } else {
        default_namespace
    };

    let buf_reader = std::io::BufReader::new(reader);
    let stream = Deserializer::from_reader(buf_reader).into_iter::<Value>();

    let mut subject_stack: VecDeque<SubjectNode> = VecDeque::new();
    let mut property: Option<String> = None;
    let mut is_root = true;

    for value in stream {
        match value {
            Ok(Value::Object(obj)) => {
                let subject = if is_root {
                    is_root = false;
                    // For the root node, create a NamedNode using blake3 hash
                    let json_str = serde_json::to_string(&obj)
                        .map_err(|_| Error::raw(clap::error::ErrorKind::Io, "JSON serialization failed"))?;
                    let hash = blake3::hash(json_str.as_bytes());
                    let hash_hex = hash.to_hex();
                    let root_uri = format!(
                        "{}#{}",
                        rdf_namespace.trim_end_matches('/').trim_end_matches('#'),
                        hash_hex
                    );
                    SubjectNode::Named(
                        NamedNode::new(root_uri)
                            .map_err(|_| Error::raw(clap::error::ErrorKind::Io, "Invalid URI"))?
                    )
                } else {
                    SubjectNode::Blank(BlankNode::default())
                };
                subject_stack.push_back(subject);

                for (key, val) in obj {
                    property = Some(format!("{}/{}", rdf_namespace, urlencoding::encode(&key)));
                    process_value(
                        &mut subject_stack,
                        &property,
                        val,
                        writer,
                        &rdf_namespace,
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
fn process_value(
    subject_stack: &mut VecDeque<SubjectNode>,
    property: &Option<String>,
    value: Value,
    writer: &mut dyn RdfWriter,
    namespace: &String,
) {
    let ns = if namespace.ends_with("/") {
        namespace
    } else {
        &([namespace, "/"].join(""))
    };

    if let Some(last_subject) = subject_stack.clone().back() {
        if let Some(prop) = property {
            match value {
                Value::Bool(b) => {
                    let literal = Literal::new_typed_literal(b.to_string(), xsd::BOOLEAN);
                    let triple = TripleRef::new(
                        &last_subject.as_ref(),
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
                    let triple = TripleRef::new(
                        &last_subject.as_ref(),
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &literal,
                    );
                    writer.add_triple(triple).unwrap();
                }
                Value::String(s) => {
                    let literal = Literal::new_typed_literal(s, xsd::STRING);
                    let triple = TripleRef::new(
                        &last_subject.as_ref(),
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &literal,
                    );
                    writer.add_triple(triple).unwrap();
                }
                Value::Null => {
                    //println!("Null value");
                }
                Value::Object(obj) => {
                    let subject = SubjectNode::Blank(BlankNode::default());
                    subject_stack.push_back(subject);

                    let triple = TripleRef::new(
                        &last_subject.as_ref(),
                        NamedNodeRef::new(prop.as_str()).unwrap(),
                        &subject_stack.back().unwrap().as_ref(),
                    );
                    writer.add_triple(triple).unwrap();

                    for (key, val) in obj {
                        let nested_property: Option<String> = Some(format!("{}{}", ns, urlencoding::encode(&key)));
                        process_value(subject_stack, &nested_property, val, writer, ns);
                    }
                    subject_stack.pop_back();
                }
                Value::Array(arr) => {
                    for val in arr {
                        process_value(subject_stack, property, val, writer, ns);
                    }
                }
            }
        }
    }
}
