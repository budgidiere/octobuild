// Wrapper for cl.exe backend. See http://blog.airesoft.co.uk/2013/01/ for more details.
extern crate winapi;
extern crate kernel32;

use crypto::digest::Digest;
use crypto::md5::Md5;
use libloading::{Library, Symbol};

use std::os::windows::ffi::OsStringExt;
use std::os::windows::ffi::OsStrExt;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::slice;

use ::cache::FileHasher;
use ::config::Config;
use ::compiler::{Hasher, OutputInfo, SharedState};

#[derive(Debug)]
pub struct BackendTask {
    inputs: Vec<PathBuf>,
    output: PathBuf,
    params: Vec<OsString>,
}

struct Suspender<'a> {
    suspend_tracking: Symbol<'a, fn() -> winapi::HRESULT>,
    resume_tracking: Symbol<'a, fn() -> winapi::HRESULT>,
}

struct SuspendHolder<'a>(&'a mut Suspender<'a>);

#[cfg(target_pointer_width = "32")]
pub const INVOKE_COMPILER_PASS_NAME: &'static [u8] = b"_InvokeCompilerPassW@16";

#[cfg(target_pointer_width = "64")]
pub const INVOKE_COMPILER_PASS_NAME: &'static [u8] = b"InvokeCompilerPassW";

#[cfg(target_pointer_width = "32")]
pub const ABORT_COMPILER_PASS_NAME: &'static [u8] = b"_AbortCompilerPass@4";

#[cfg(target_pointer_width = "64")]
pub const ABORT_COMPILER_PASS_NAME: &'static [u8] = b"AbortCompilerPass";

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
    invoke_compiler_pass_wrapper(args, || {
        singleton_c2()
            .and_then(|lib| unsafe { lib.get::<FnInvokeCompilerPass>(INVOKE_COMPILER_PASS_NAME) })
            .map(|func| (func)(argc, argv, unknown, cluimod))
            .unwrap_or_else(|e| {
                error!("Can't execute original function {}: {}",
                       String::from_utf8_lossy(INVOKE_COMPILER_PASS_NAME),
                       e);
                0xFFFE
            })
    })
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
            let mut tracker = singleton_file_tracker()
                .and_then(|lib| Suspender::new(lib))
                .map_err(|e| {
                    warn!("Can't use FileTracker object");
                })
                .ok();
            let suspend_holder = tracker.as_mut().map(|mut t| t.suspend());
            let result = match state.cache.run_file_cached(&state.statistic,
                                                           &hash,
                                                           &vec![task.output],
                                                           || -> Result<OutputInfo, Error> {
                Ok(OutputInfo {
                    status: Some(original() as i32),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            },
                                                           || true) {
                Ok(output) => output.status.unwrap() as u32,
                Err(e) => {
                    warn!("Can't run original backend with cache: {}", e);
                    0xFE
                }
            };
            drop(suspend_holder);
            result
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
            for suffix in [// "gl", "db", // todo #22
                           "ex",
                           "in",
                           "sy"]
                .iter() {
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
    singleton_c2()
        .and_then(|lib| unsafe { lib.get::<FnAbortCompilerPass>(ABORT_COMPILER_PASS_NAME) })
        .map(|func| (func)(how))
        .unwrap_or_else(|e| {
            info!("Can't execute original function {}: {}",
                  String::from_utf8_lossy(ABORT_COMPILER_PASS_NAME),
                  e);
        })
}

fn singleton_c2() -> Result<&'static Library, Error> {
    fn path() -> PathBuf {
        env::current_exe().map(|path| path.with_file_name("c2.dll")).unwrap()
    }
    fn create() -> Option<Library> {
        Library::new(path()).ok()
    }
    lazy_static! {
		static ref SINGLETON: Option<Library> =create() ;
	}
    SINGLETON.as_ref().ok_or(Error::new(ErrorKind::NotFound,
                                        format!("Can't load shared library: {:?}", path())))
}

fn singleton_file_tracker() -> Result<&'static Library, Error> {
    fn path() -> PathBuf {
        Path::new("FileTracker.dll").to_path_buf()
    }
    fn create() -> Option<Library> {
        Library::new(path()).ok()
    }
    lazy_static! {
		static ref SINGLETON: Option<Library> =create() ;
	}
    SINGLETON.as_ref().ok_or(Error::new(ErrorKind::NotFound,
                                        format!("Can't load shared library: {:?}", path())))
}

fn singleton_state() -> Option<&'static SharedState> {
    fn create() -> Option<SharedState> {
        let config = match Config::new() {
            Ok(v) => v,
            Err(e) => {
                error!("Can't create shared state: {:?}", e);
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

impl<'a> Suspender<'a> {
    fn new(lib: &'a Library) -> Result<Self, Error> {
        Ok(Suspender {
            // https://msdn.microsoft.com/en-us/library/ee904207.aspx
            suspend_tracking: try!(unsafe { lib.get(b"SuspendTracking") }),
            // https://msdn.microsoft.com/en-us/library/ee904206.aspx
            resume_tracking: try!(unsafe { lib.get(b"ResumeTracking") }),
        })
    }

    fn suspend(&'a mut self) -> SuspendHolder<'a> {
        (self.suspend_tracking)();
        SuspendHolder(self)
    }
}

impl<'a> Drop for SuspendHolder<'a> {
    fn drop(&mut self) {
        (self.0.resume_tracking)();
    }
}
