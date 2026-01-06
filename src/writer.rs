// Copyright (c) 2024-2025, DeciSym, LLC
// Licensed under either of:
// - Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// - BSD 3-Clause License (https://opensource.org/licenses/BSD-3-Clause)
// at your option.

//! # JSON2RDF Writer Library
//!
//! This library provides functionality for writing converted JSON2RDF data.
//! It uses `oxrdf` to build and manage RDF graphs or output the data direct to a file.
//!
//! ## Overview
//! - Adds JSON RDF triples to a graph or file.

use oxrdf::{Graph, TripleRef};
use std::io::{self, BufWriter, Write};

pub trait RdfWriter {
    fn add_triple(&mut self, triple: TripleRef) -> std::io::Result<()>;
}

pub struct FileWriter<W: Write> {
    writer: BufWriter<W>,
}

impl FileWriter<io::Stdout> {
    /// Create a FileWriter that writes to stdout.
    pub fn to_stdout() -> Self {
        FileWriter {
            writer: BufWriter::new(io::stdout()),
        }
    }
}

impl FileWriter<Vec<u8>> {
    /// Create a FileWriter that writes into an in-memory buffer (Vec<u8>).
    /// Useful in wasm or tests where a filesystem is not available.
    pub fn to_vec() -> Self {
        FileWriter {
            writer: BufWriter::new(Vec::new()),
        }
    }

    /// Consume the FileWriter and return the inner Vec<u8> buffer.
    pub fn into_vec(self) -> Vec<u8> {
        match self.writer.into_inner() {
            Ok(v) => v,
            Err(e) => match e.into_inner().into_inner() {
                Ok(v2) => v2,
                Err(_) => Vec::new(),
            },
        }
    }
}

impl<W: Write> FileWriter<W> {
    /// Create a FileWriter from any writer implementing `Write`.
    /// This replaces direct filesystem constructors and makes the
    /// writer usable in wasm environments.
    pub fn new(writer: W) -> Self {
        FileWriter {
            writer: BufWriter::new(writer),
        }
    }

    // Generic extraction of the inner writer is not provided because
    // `BufWriter::into_inner` can fail and handling depends on caller needs.
}

impl<W: Write> RdfWriter for FileWriter<W> {
    fn add_triple(&mut self, triple: TripleRef) -> std::io::Result<()> {
        self.writer.write_all(triple.to_string().as_bytes())?;
        self.writer.write_all(b" .\n")?;
        let _ = self.writer.flush();
        Ok(())
    }
}

pub struct GraphWriter<'a> {
    graph: &'a mut Graph,
}

impl<'a> GraphWriter<'a> {
    pub fn new(graph: &'a mut Graph) -> Self {
        Self { graph }
    }
}

impl RdfWriter for GraphWriter<'_> {
    fn add_triple(&mut self, triple: TripleRef) -> std::io::Result<()> {
        self.graph.insert(triple);
        Ok(())
    }
}
