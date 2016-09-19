extern crate clap;
extern crate octobuild;

use clap::{App, AppSettings, Arg};
use std::io::{Cursor, Error, Read, Write};
use std::fs::File;

use self::octobuild::version::{AUTHORS, VERSION};
use self::octobuild::vs::postprocess;
use self::octobuild::vs::postprocess::PostprocessWrite;

struct OutputWrapper {
    content: Vec<u8>,
}

impl Write for OutputWrapper {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.content.write(buf)
    }
    fn flush(&mut self) -> Result<(), Error> {
        self.content.flush()
    }
}

impl PostprocessWrite for OutputWrapper {
    fn is_source_separator(&mut self, marker: &[u8]) -> Result<bool, Error> {
        try!(self.content.write(b"/// "));
        try!(self.content.write(marker));
        try!(self.content.write(b"\n"));
        Ok(false)
    }
}

fn bench_filter(path: &str, marker: &Option<String>, keep_headers: bool, num: usize) -> Vec<u8> {
    let mut source = Vec::new();
    File::open(path).unwrap().read_to_end(&mut source).unwrap();

    let mut total: usize = 0;
    let mut result = OutputWrapper { content: Vec::with_capacity(source.len()) };
    for _ in 0..num {
        result.content.clear();
        postprocess::filter_preprocessed(&mut Cursor::new(source.clone()),
                                         &mut result,
                                         true,
                                         &marker,
                                         keep_headers)
            .unwrap();
        total += result.content.len();
    }
    assert_eq!(total / num, result.content.len());
    result.content
}

fn main() {
    const MARKER: &'static str = "marker";
    const INPUT: &'static str = "input";
    const COUNT: &'static str = "count";
    const KEEP: &'static str = "keep";

    let matches = App::new("filter_cl")
        .setting(AppSettings::ArgRequiredElseHelp)
        .version(VERSION)
        .author(AUTHORS)
        .about("Preprocessor filter for CL.exe compiler test tool")
        .arg(Arg::with_name(MARKER)
            .short("m")
            .long("marker")
            .value_name("header")
            .takes_value(true)
            .help("Precompiled header marker (like StdAfx.h)"))
        .arg(Arg::with_name(KEEP)
            .short("k")
            .long("keep")
            .help("Keep header before precompiled header marker"))
        .arg(Arg::with_name(COUNT)
            .short("c")
            .long("count")
            .default_value("1")
            .help("Iteration count"))
        .arg(Arg::with_name(INPUT)
            .required(true)
            .index(1)
            .help("Preprocessed input file"))
        .get_matches();

    let inputs = matches.values_of_lossy(INPUT).unwrap();
    let marker = matches.value_of(MARKER).map(|s| s.to_string());
    let keep = matches.is_present(KEEP);
    let count = matches.value_of(COUNT).unwrap_or("1").parse::<usize>().unwrap();

    for input in inputs.iter() {
        bench_filter(input, &marker, keep, count);
    }
}
