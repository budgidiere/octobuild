// Wrapper for cl.exe backend. See http://blog.airesoft.co.uk/2013/01/ for more details.
extern crate winapi;

#[cfg(target_pointer_width = "32")]
pub const INVOKE_COMPILER_PASS_NAME: &'static str = "_InvokeCompilerPassW@16";

#[cfg(target_pointer_width = "64")]
pub const INVOKE_COMPILER_PASS_NAME: &'static str = "InvokeCompilerPassW";

#[cfg(target_pointer_width = "32")]
pub const ABORT_COMPILER_PASS_NAME: &'static str = "_AbortCompilerPass@4";

#[cfg(target_pointer_width = "64")]
pub const ABORT_COMPILER_PASS_NAME: &'static str = "AbortCompilerPass";

// BOOL __stdcall InvokeCompilerPassW(int argc, wchar_t** argv, int unk, HMODULE* phCLUIMod) // exported as _InvokeCompilerPassW@16
#[cfg(target_pointer_width = "32")]
#[export_name = "_InvokeCompilerPassW"]
pub extern "stdcall" fn invoke_compiler_pass_extern(argc: winapi::DWORD,
                                                    argv: *mut winapi::LPCWSTR,
                                                    unknown: winapi::DWORD,
                                                    cluimod: *const winapi::HMODULE)
                                                    -> winapi::BOOL {
    invoke_compiler_pass(argc, argv, unknown, cluimod)
}

#[cfg(target_pointer_width = "64")]
#[export_name = "InvokeCompilerPassW"]
pub extern "stdcall" fn invoke_compiler_pass_extern(argc: winapi::DWORD,
                                                    argv: *mut winapi::LPCWSTR,
                                                    unknown: winapi::DWORD,
                                                    cluimod: *const winapi::HMODULE)
                                                    -> winapi::BOOL {
    invoke_compiler_pass(argc, argv, unknown, cluimod)
}

// void WINAPI AbortCompilerPass(int how) // exported as _AbortCompilerPass@4
#[cfg(target_pointer_width = "32")]
#[export_name = "_AbortCompilerPass"]
pub extern "stdcall" fn abort_compiler_pass_extern(how: winapi::DWORD) {
    abort_compiler_pass(how)
}
#[cfg(target_pointer_width = "64")]
#[export_name = "AbortCompilerPass"]
pub extern "stdcall" fn abort_compiler_pass_extern(how: winapi::DWORD) {
    abort_compiler_pass(how)
}

fn invoke_compiler_pass(argc: winapi::DWORD,
                        argv: *mut winapi::LPCWSTR,
                        unknown: winapi::DWORD,
                        cluimod: *const winapi::HMODULE)
                        -> winapi::BOOL {
    println!("ARGC: {}", argc);
    0
}

fn abort_compiler_pass(how: winapi::DWORD) {}

#[cfg(test)]
mod test {
    extern crate kernel32;

    use std::env;
    use std::ptr;
    use std::ffi::CString;
    use std::os::windows::ffi::OsStrExt;

    use super::{ABORT_COMPILER_PASS_NAME, INVOKE_COMPILER_PASS_NAME};

    fn check_function_exists(name: &str) {
        let library_path = env::current_exe().unwrap().with_file_name("octobuild.dll");
        println!("Check function {} for library {:?}", name, library_path);
        assert!(library_path.is_file());
        unsafe {
            let library = kernel32::LoadLibraryW(library_path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
                .as_ptr());
            assert!(library != ptr::null_mut());
            let address = kernel32::GetProcAddress(library, CString::new(name).unwrap().as_ptr());
            kernel32::FreeLibrary(library);
            assert!(address != ptr::null_mut());
        }
    }

    #[test]
    fn test_get_module_handle_address_exists() {
        check_function_exists(INVOKE_COMPILER_PASS_NAME)
    }

    #[test]
    fn test_abort_compiler_pass_exists() {
        check_function_exists(ABORT_COMPILER_PASS_NAME)
    }
}
