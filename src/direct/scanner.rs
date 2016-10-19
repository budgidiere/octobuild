use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::ffi::{OsStr, OsString};
use std::io::{Error, ErrorKind, Read};
use std::iter;
use std::rc::Rc;
use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};

use ::filter::includes::{Include, source_includes};

#[derive(Clone)]
#[derive(Hash)]
#[derive(Eq)]
#[derive(PartialEq)]
struct CollectTask {
    context: Rc<Vec<PathBuf>>,
    source: PathBuf,
}

pub trait IncludeCombine {
    fn combine_context(&self, context: &Rc<Vec<PathBuf>>, source_path: &Path) -> Rc<Vec<PathBuf>>;
}

pub trait IncludeState {
    fn file_includes(&mut self, source_file: &Path) -> Result<Option<Rc<IncludeInfo>>, Error>;
}

pub struct IncludeInfo {
    pub canonical_path: PathBuf,
    pub includes: Vec<Include<String>>,
}

pub struct IncludeReader {}

pub struct IncludeCacher<T: IncludeState> {
    state: T,
    cache: HashMap<PathBuf, Option<Rc<IncludeInfo>>>,
}

pub enum IncludeBehaviour {
    Clang,
    VisualStudio,
}

pub fn collect_includes<T, I>(state: &mut T,
                              source_file: &Path,
                              include_dir: &[I],
                              combine: &IncludeCombine)
                              -> Result<HashSet<PathBuf>, Error>
    where T: IncludeState,
          I: AsRef<Path>
{
    let mut queue: Vec<(CollectTask, Rc<IncludeInfo>)> = Vec::new();
    let mut result: HashSet<CollectTask> = HashSet::new();

    match try!(state.file_includes(source_file)) {
        Some(include) => {
            queue.push((CollectTask {
                context: combine.combine_context(&Rc::new(Vec::new()), &include.canonical_path),
                source: include.canonical_path.clone(),
            },
                        include));
        }
        None => {
            return Err(Error::new(ErrorKind::NotFound,
                                  source_file.to_string_lossy().to_string()));
        }
    }
    loop {
        match queue.pop() {
            Some((task, source_info)) => {
                if result.insert(task.clone()) {
                    for include in try!(file_include_paths(state, &source_info, &task.context[..], include_dir))
                        .into_iter() {
                        queue.push((CollectTask {
                            context: combine.combine_context(&task.context, &include.canonical_path),
                            source: include.canonical_path.clone(),
                        },
                                    include));
                    }
                }
            }
            None => {
                break;
            }
        }
    }
    Ok(result.into_iter().map(|t| t.source).collect())
}

impl IncludeCombine for IncludeBehaviour {
    fn combine_context(&self, context: &Rc<Vec<PathBuf>>, source_path: &Path) -> Rc<Vec<PathBuf>> {
        match self {
            &IncludeBehaviour::Clang => Rc::new(source_path.parent().map(|p| p.to_path_buf()).into_iter().collect()),
            &IncludeBehaviour::VisualStudio => {
                match source_path.parent() {
                    Some(source_dir) => {
                        if context.last().map_or(false, |v| v.as_path() == source_dir) {
                            return context.clone();
                        }
                        Rc::new(context.iter()
                            .filter(|v| v.as_path() != source_dir)
                            .map(|v| v.clone())
                            .chain(iter::once(source_dir.to_path_buf()))
                            .collect())
                    }
                    None => context.clone(),
                }
            }
        }
    }
}

impl IncludeReader {
    pub fn new() -> Self {
        IncludeReader {}
    }
}

impl IncludeCacher<IncludeReader> {
    pub fn default() -> Self {
        IncludeCacher::new(IncludeReader::new())
    }
}

impl<T: IncludeState> IncludeCacher<T> {
    pub fn new(state: T) -> Self {
        IncludeCacher {
            state: state,
            cache: HashMap::new(),
        }
    }
}

impl IncludeState for IncludeReader {
    fn file_includes(&mut self, source_file: &Path) -> Result<Option<Rc<IncludeInfo>>, Error> {
        let canonical_path = match normalize_path(source_file) {
            Ok(v) => v,
            Err(e) => {
                return match e.kind() {
                    ErrorKind::NotFound => Ok(None),
                    _ => Err(e),
                }
            }
        };
        let mut file = match File::open(&canonical_path) {
            Ok(v) => v,
            Err(e) => {
                return match e.kind() {
                    ErrorKind::NotFound => Ok(None),
                    _ => Err(e),
                }
            }
        };
        println!("> LOAD: {:?}", canonical_path);
        let mut buffer = Vec::new();
        try!(file.read_to_end(&mut buffer));
        source_includes(&buffer).map(|v| {
            Some(Rc::new(IncludeInfo {
                canonical_path: canonical_path,
                includes: v,
            }))
        })
    }
}

impl<T: IncludeState> IncludeState for IncludeCacher<T> {
    fn file_includes(&mut self, source_file: &Path) -> Result<Option<Rc<IncludeInfo>>, Error> {
        let path = try!(normalize_path(source_file));
        match self.cache.entry(path) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => self.state.file_includes(entry.key()).map(|e| entry.insert(e).clone()),
        }
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, Error> {
    let mut result = OsString::with_capacity(path.as_os_str().len());
    let mut components = Vec::new();
    for iter in path.components() {
        match iter {
            Component::RootDir => {
                result.push(Component::RootDir.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err(Error::new(ErrorKind::Other, format!("Invalid input path: {:?}", path)));
                }
            }
            Component::Normal(p) => {
                components.push(p);
            }
            Component::Prefix(p) => {
                result.push(p.as_os_str());
            }
        }
    }
    let mut separator = false;
    for iter in components.iter() {
        if separator {
            result.push(OsStr::new(&MAIN_SEPARATOR.to_string()));
        } else {
            separator = true;
        }
        result.push(iter);
    }
    Ok(Path::new(&result).to_path_buf())
}

fn file_include_paths<T, I>(state: &mut T,
                            info: &IncludeInfo,
                            context_dir: &[PathBuf],
                            include_dir: &[I])
                            -> Result<Vec<Rc<IncludeInfo>>, Error>
    where T: IncludeState,
          I: AsRef<Path>
{
    info.includes
        .iter()
        .filter_map(|include| {
            let result = match include {
                &Include::Quote(ref name) => {
                    solve_include_path(state,
                                       Path::new(name),
                                       context_dir.iter()
                                           .rev()
                                           .map(|p| p.as_path())
                                           .chain(include_dir.iter().map(|p| p.as_ref())))
                }
                &Include::Bracket(ref name) => {
                    solve_include_path(state,
                                       Path::new(name),
                                       include_dir.iter().map(|p| p.as_ref()))
                }
            };
            match result {
                Ok(v) => v.map(|v| Ok(v)),
                Err(e) => Some(Err(e)),
            }
        })
        .collect()
}

fn solve_include_path<'a, T, I>(state: &mut T,
                                include_path: &Path,
                                dir_iter: I)
                                -> Result<Option<Rc<IncludeInfo>>, Error>
    where T: IncludeState,
          I: Iterator<Item = &'a Path>
{
    if include_path.is_absolute() {
        return state.file_includes(include_path);
    }
    for dir in dir_iter {
        match try!(state.file_includes(&dir.join(include_path))) {
            Some(v) => {
                return Ok(Some(v));
            }
            None => {}
        }
    }
    Ok(None)
}
