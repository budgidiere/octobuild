/** This binary used for testing ocobuild.dll function export names on Linux with Wine.
*/
extern crate octobuild;
extern crate libloading;

#[cfg(windows)]
use octobuild::vs::c2::*;

#[cfg(windows)]
fn check_function_exists<F>(name: &[u8], _: F) {
    use std::env;
    use libloading::Library;

    let library_path = env::current_exe().unwrap().with_file_name("octobuild.dll");
    println!("Check function {} for library {:?}",
             String::from_utf8_lossy(name),
             library_path);
    assert!(library_path.is_file());

    let lib = Library::new(library_path).unwrap();
    assert!(unsafe { lib.get::<F>(name) }.is_ok());
}

#[cfg(windows)]
fn main() {
    check_function_exists::<FnAbortCompilerPass>(ABORT_COMPILER_PASS_NAME, abort_compiler_pass_extern);
    check_function_exists::<FnInvokeCompilerPass>(INVOKE_COMPILER_PASS_NAME, invoke_compiler_pass_extern);
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
#[test]
fn test_invoke_compiler_pass_exists() {
    check_function_exists::<FnInvokeCompilerPass>(INVOKE_COMPILER_PASS_NAME, invoke_compiler_pass_extern)
}

#[cfg(windows)]
#[test]
fn test_abort_compiler_pass_exists() {
    check_function_exists::<FnAbortCompilerPass>(ABORT_COMPILER_PASS_NAME, abort_compiler_pass_extern)
}
