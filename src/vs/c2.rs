// Wrapper for cl.exe backend. See http://blog.airesoft.co.uk/2013/01/ for more details.
extern crate winapi;
extern crate kernel32;

use crypto::digest::Digest;
use crypto::md5::Md5;

use std::os::windows::ffi::OsStringExt;
use std::os::windows::ffi::OsStrExt;
use std::env;
use std::ffi::{CString, OsStr, OsString};
use std::io::{Error, ErrorKind};
use std::mem;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;

use ::cache::FileHasher;
use ::config::Config;
use ::compiler::{Hasher, OutputInfo, SharedState};

pub struct Library {
    handle: winapi::HMODULE,
    auto_unload: bool,
}

pub struct LibraryC2 {
    invoke_compiler_pass: FnInvokeCompilerPass,
    abort_compiler_pass: FnAbortCompilerPass,
}

#[derive(Debug)]
pub struct BackendTask {
    inputs: Vec<PathBuf>,
    output: PathBuf,
    params: Vec<OsString>,
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
    invoke_compiler_pass_wrapper(args,
                                 || (singleton_c2().invoke_compiler_pass)(argc, argv, unknown, cluimod))
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

fn generate_hash(state: &SharedState, task: &BackendTask) -> Result<String, Error> {
    let mut hasher = Md5::new();
    // Hash parameters
    hasher.hash_u64(task.params.len() as u64);
    for iter in task.params.iter() {
        hash_os_str(&mut hasher, &iter);
    }
    // Hash input files
    hasher.hash_u64(task.inputs.len() as u64);
    for path in task.inputs.iter() {
        hasher.hash_bytes(try!(state.cache.file_hash(&path)).hash.as_bytes());
    }
    Ok(hasher.result_str())
}

fn hash_os_str<H: Hasher>(hasher: &mut H, value: &OsStr) {
    hasher.hash_u64(value.len() as u64);
    for i in value.encode_wide() {
        hasher.hash_bytes(&[(i & 0xFF) as u8, ((i >> 8) & 0xFF) as u8]);
    }
}

fn invoke_compiler_pass_wrapper<F>(args: Vec<OsString>, original: F) -> u32
    where F: FnOnce() -> u32
{
    match prepare_task(args) {
        Ok(task) => {
            let state = match singleton_state() {
                Some(v) => v,
                None => {
                    error!("FATAL ERROR: Can't initialize octobuild");
                    return original();
                }
            };
            let hash = match generate_hash(&state, &task) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Can't generate hash for compilation task: {}", e);
                    return original();
                }
            };
            match state.cache.run_file_cached(&state.statistic,
                                              &hash,
                                              &vec![task.output],
                                              || -> Result<OutputInfo, Error> {
                Ok(OutputInfo {
                    status: Some(original() as i32),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            },                                              || true) {
                Ok(output) => output.status.unwrap() as u32,
                Err(e) => {
                    warn!("Can't run original backend with cache: {}", e);
                    0xFE
                }
            }
        }
        Err(e) => {
            info!("Can't use octobuild for task: {}", e);
            original()
        }
    }
}

fn prepare_task(args: Vec<OsString>) -> Result<BackendTask, String> {
    let mut inputs = Vec::new();
    let mut output = None;
    let mut params = Vec::new();
    let mut iter = args.into_iter();
    // Skip program name
    if iter.next().is_none() {
        return Err("Empty arguments list".to_string());
    }
    loop {
        let arg = match iter.next() {
            Some(v) => v,
            None => break,
        };
        if arg == OsStr::new("-il") {
            let base = try!(iter.next().ok_or("Can't get -il key value".to_string()));
            for suffix in ["db", "ex", "gl", "in"].iter() {
                let mut path = base.clone();
                path.push(OsStr::new(suffix));
                inputs.push(Path::new(path.as_os_str()).to_path_buf());
            }
            continue;
        }
        if arg == OsStr::new("-MPdiagMutex") {
            try!(iter.next().ok_or("Can't get -MPdiagMutex key value".to_string()));
            continue;
        }
        let vec = arg.encode_wide().collect::<Vec<_>>();
        if vec.starts_with(&['-' as u16, 'F' as u16, 'o' as u16]) {
            if output.is_some() {
                return Err("Multiple output files is not supported".to_string());
            }
            output = Some(Path::new(OsString::from_wide(&vec[3..]).as_os_str()).to_path_buf());
            continue;
        }
        params.push(arg);
    }
    if inputs.is_empty() {
        return Err("Don't find input file list".to_string());
    }
    Ok(BackendTask {
        inputs: inputs,
        output: try!(output.ok_or("Don't find output file name".to_string())),
        params: params,
    })
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
    (singleton_c2().abort_compiler_pass)(how)
}

extern "stdcall" fn abort_compiler_pass_fallback(_: winapi::DWORD) {
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

fn singleton_c2() -> &'static LibraryC2 {
    fn create() -> LibraryC2 {
        env::current_exe()
            .map(|path| LibraryC2::load(&path.with_file_name("c2.dll"), false))
            .unwrap_or_else(|_| LibraryC2::fallback())
    }
    lazy_static! {
		static ref SINGLETON: LibraryC2  =create() ;
	}
    &SINGLETON
}

fn singleton_state() -> Option<&'static SharedState> {
    fn create() -> Option<SharedState> {
        let config = match Config::new() {
            Ok(v) => v,
            Err(e) => {
                return None;
            }
        };
        Some(SharedState::new(&config))
    }

    lazy_static! {
		static ref SINGLETON:  Option<SharedState>  = create();
	}
    SINGLETON.as_ref()
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
