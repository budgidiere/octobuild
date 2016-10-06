extern crate octobuild;

use octobuild::filter::includes::{Include, find_includes};

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;

fn main() {
    for arg in env::args().skip(1) {
        println!("File: {}", arg);
        let buffer = File::open(Path::new(&arg))
            .and_then(|mut f| {
                let mut v = Vec::new();
                f.read_to_end(&mut v).map(|_| v)
            })
            .unwrap();
        let res = find_includes(&buffer).map(|(bom, v)| {
            (bom,
             v.into_iter()
                .map(|v| v.map(|o| -> String { String::from_utf8(o).unwrap() }))
                .collect::<Vec<Include<String>>>())
        });
        println!("{:?}", res);
    }
}
