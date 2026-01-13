// Copyright (c) 2024-2025, DeciSym, LLC
// Licensed under either of:
// - Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// - BSD 3-Clause License (https://opensource.org/licenses/BSD-3-Clause)
// at your option.

use json2rdf::json_to_rdf;
use json2rdf::writer::FileWriter;
use oxrdfio::{RdfFormat, RdfParser};
use std::fs::{self, File};
use std::io::Cursor;

#[test]
fn test_graph_triple_count() {
    let f = File::open("tests/airplane.json").expect("unable to open test json");
    let mut w = FileWriter::to_vec();

    let res = json_to_rdf(f, &mut w, &None);
    assert!(res.is_ok());

    let out_bytes = w.into_vec();
    let cursor = Cursor::new(out_bytes);
    let quads = RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(cursor)
        .collect::<Result<Vec<_>, _>>()
        .expect("failed to parse generated output");

    assert_eq!(quads.len(), 23);
}

#[test]
fn test_graph_write() {
    let output = "out.nt".to_string();
    let _ = fs::remove_file(output.clone());
    let infile = File::open("tests/airplane.json").expect("unable to open test json");
    let outfile = File::create(output.clone()).expect("unable to create output file");

    let mut writer = FileWriter::new(outfile);
    let res = json_to_rdf(infile, &mut writer, &None);

    assert!(res.is_ok());

    // close and reopen file to parse
    drop(writer);
    let f = File::open(output.clone()).expect("unable to open output file for result verification");
    let quads = RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(f)
        .collect::<Result<Vec<_>, _>>()
        .expect("failed to parse generated output file");

    assert_eq!(quads.len(), 23);
    let _ = fs::remove_file(output.clone());
}
