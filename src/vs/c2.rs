// Wrapper for cl.exe backend. See http://blog.airesoft.co.uk/2013/01/ for more details.
extern crate winapi;
extern crate kernel32;

use std::os::windows::ffi::OsStringExt;
use std::os::windows::ffi::OsStrExt;
use std::env;
use std::ffi::{CString, OsString};
use std::io::{Error, ErrorKind};
use std::mem;
use std::path::Path;
use std::ptr;
use std::slice;

pub struct Library {
    handle: winapi::HMODULE,
    auto_unload: bool,
}

pub struct LibraryC2 {
    invoke_compiler_pass: FnInvokeCompilerPass,
    abort_compiler_pass: FnAbortCompilerPass,
}

#[cfg(target_pointer_width = "32")]
pub const INVOKE_COMPILER_PASS_NAME: &'static str = "_InvokeCompilerPassW@16";

#[cfg(target_pointer_width = "64")]
pub const INVOKE_COMPILER_PASS_NAME: &'static str = "InvokeCompilerPassW";

#[cfg(target_pointer_width = "32")]
pub const ABORT_COMPILER_PASS_NAME: &'static str = "_AbortCompilerPass@4";

#[cfg(target_pointer_width = "64")]
pub const ABORT_COMPILER_PASS_NAME: &'static str = "AbortCompilerPass";

pub type FnInvokeCompilerPass = extern "stdcall" fn(winapi::DWORD,
                                                    *mut winapi::LPCWSTR,
                                                    winapi::DWORD,
                                                    *const winapi::HMODULE)
                                                    -> winapi::DWORD;
pub type FnAbortCompilerPass = extern "stdcall" fn(winapi::DWORD);

// BOOL __stdcall InvokeCompilerPassW(int argc, wchar_t** argv, int unk, HMODULE* phCLUIMod) // exported as _InvokeCompilerPassW@16
#[cfg(target_pointer_width = "32")]
#[export_name = "_InvokeCompilerPassW"]
pub extern "stdcall" fn invoke_compiler_pass_extern(argc: winapi::DWORD,
                                                    argv: *mut winapi::LPCWSTR,
                                                    unknown: winapi::DWORD,
                                                    cluimod: *const winapi::HMODULE)
                                                    -> winapi::DWORD {
    invoke_compiler_pass(argc, argv, unknown, cluimod)
}

#[cfg(target_pointer_width = "64")]
#[export_name = "InvokeCompilerPassW"]
pub extern "stdcall" fn invoke_compiler_pass_extern(argc: winapi::DWORD,
                                                    argv: *mut winapi::LPCWSTR,
                                                    unknown: winapi::DWORD,
                                                    cluimod: *const winapi::HMODULE)
                                                    -> winapi::DWORD {
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
                        -> winapi::DWORD {
    let argv_slice = unsafe { slice::from_raw_parts(argv, argc as usize) };
    let mut args = Vec::with_capacity(argc as usize);
    for i in 0..argc {
        args.push(unsafe { OsString::from_wide_ptr(argv_slice[i as usize]) });
    }
    println!("EXE: {:?}\n{:?}", env::current_exe(), args);
    invoke_compiler_pass_wrapper(&[], || {
        (c2().invoke_compiler_pass)(argc, argv, unknown, cluimod)
    })
}

extern "stdcall" fn invoke_compiler_pass_fallback(_: winapi::DWORD,
                                                  _: *mut winapi::LPCWSTR,
                                                  _: winapi::DWORD,
                                                  _: *const winapi::HMODULE)
                                                  -> winapi::DWORD {
    error!("Can't find function in C2.dll compiler: {}",
           INVOKE_COMPILER_PASS_NAME);
    return 0xFF;
}

fn invoke_compiler_pass_wrapper<F>(args: &[OsString], original: F) -> u32
    where F: FnOnce() -> u32
{
    println!("BEGIN");
    let r = original();
    println!("END");
    r
}

pub trait OsStringExt2 {
    /// Creates an `OsString` from a potentially ill-formed UTF-16 slice of
    /// 16-bit code units.
    ///
    /// This is lossless: calling `.encode_wide()` on the resulting string
    /// will always return the original code units.
    unsafe fn from_wide_ptr(wide: *const u16) -> Self;
}

impl OsStringExt2 for OsString {
    unsafe fn from_wide_ptr(ptr: *const u16) -> OsString {
        let mut len = 0;
        while *ptr.offset(len) != 0 {
            len += 1;
        }
        OsString::from_wide(slice::from_raw_parts(ptr, len as usize))
    }
}

fn abort_compiler_pass(how: winapi::DWORD) {
    (c2().abort_compiler_pass)(how)
}

extern "stdcall" fn abort_compiler_pass_fallback(how: winapi::DWORD) {
    error!("Can't find function in C2.dll compiler: {}",
           ABORT_COMPILER_PASS_NAME);
}

impl Library {
    fn load(path: &Path, auto_unload: bool) -> Result<Self, Error> {
        let handle = unsafe {
            kernel32::LoadLibraryW(path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
                .as_ptr())
        };
        if handle == ptr::null_mut() {
            Err(Error::last_os_error())
        } else {
            Ok(Library {
                handle: handle,
                auto_unload: auto_unload,
            })
        }
    }

    fn lookup(&self, name: &str) -> Result<usize, Error> {
        unsafe {
            let address = kernel32::GetProcAddress(self.handle, try!(CString::new(name)).as_ptr());
            if address == ptr::null_mut() {
                Err(Error::new(ErrorKind::NotFound, name))
            } else {
                Ok(address as usize)
            }
        }
    }
}

unsafe impl Sync for Library {}

impl Drop for Library {
    fn drop(&mut self) {
        if self.auto_unload {
            unsafe {
                kernel32::FreeLibrary(self.handle);
            }
        }
    }
}

impl LibraryC2 {
    fn load(path: &Path, auto_unload: bool) -> Self {
        Library::load(path, auto_unload)
            .map(|library| {
                let fallback = LibraryC2::fallback();
                LibraryC2 {
                    invoke_compiler_pass: library.lookup(INVOKE_COMPILER_PASS_NAME)
                        .map(|addr| unsafe { mem::transmute::<usize, FnInvokeCompilerPass>(addr) })
                        .unwrap_or(fallback.invoke_compiler_pass),
                    abort_compiler_pass: library.lookup(ABORT_COMPILER_PASS_NAME)
                        .map(|addr| unsafe { mem::transmute::<usize, FnAbortCompilerPass>(addr) })
                        .unwrap_or(fallback.abort_compiler_pass),
                }
            })
            .unwrap_or_else(|_| LibraryC2::fallback())
    }

    fn fallback() -> Self {
        LibraryC2 {
            invoke_compiler_pass: invoke_compiler_pass_fallback,
            abort_compiler_pass: abort_compiler_pass_fallback,
        }
    }
}

fn c2() -> &'static LibraryC2 {
    lazy_static! {
		static ref L: LibraryC2  = env::current_exe()
		.map(|path| LibraryC2::load(& path.with_file_name("c2.dll"), false)		)
		.unwrap_or_else(|_| LibraryC2::fallback());
	}
    &L
}

#[cfg(test)]
fn check_function_exists<F>(name: &str, _: F) {
    let library_path = env::current_exe().unwrap().with_file_name("octobuild.dll");
    println!("Check function {} for library {:?}", name, library_path);
    assert!(library_path.is_file());
    let library = Library::load(&library_path, true).unwrap();
    assert!(library.lookup(name).is_ok());
}

#[test]
fn test_invoke_compiler_pass_exists() {
    check_function_exists::<FnInvokeCompilerPass>(INVOKE_COMPILER_PASS_NAME, invoke_compiler_pass_extern)
}

#[test]
fn test_abort_compiler_pass_exists() {
    check_function_exists::<FnAbortCompilerPass>(ABORT_COMPILER_PASS_NAME, abort_compiler_pass_extern)
}
