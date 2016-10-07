extern crate octobuild;

use octobuild::filter::includes::source_includes;
use octobuild::direct::scanner::collect_includes;

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
        //let res = source_includes(&buffer);
        let res = collect_includes(Path::new(&arg), &[
            Path::new("/usr/bin/../lib/gcc/x86_64-linux-gnu/5.4.0/../../../../include/c++/5.4.0"),
            Path::new("/usr/bin/../lib/gcc/x86_64-linux-gnu/5.4.0/../../../../include/x86_64-linux-gnu/c++/5.4.0"),
            Path::new("/usr/bin/../lib/gcc/x86_64-linux-gnu/5.4.0/../../../../include/c++/5.4.0/backward"),
            Path::new("/usr/local/include"),
            Path::new("/usr/lib/llvm-3.8/bin/../lib/clang/3.8.0/include"),
            Path::new("/usr/include/x86_64-linux-gnu"),
            Path::new("/usr/include"),
        ]);
        println!("{:?}", res);
    }
}
