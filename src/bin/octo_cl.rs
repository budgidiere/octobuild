extern crate octobuild;
extern crate tempdir;

use tempdir::TempDir;

use octobuild::vs::compiler::VsCompiler;
use octobuild::simple::simple_compile;

use std::process;
use std::sync::Arc;

fn main() {
    process::exit(simple_compile("cl.exe", |config| Ok(VsCompiler::new(&Arc::new(try!(TempDir::new("octobuild"))), config.preprocess_batch))))
}
