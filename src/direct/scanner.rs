use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Error, Read};
use std::iter;
use std::rc::Rc;
use std::path::{Path, PathBuf};

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
    fn file_includes(&mut self, source_file: &Path) -> Result<Rc<Vec<Include<String>>>, Error>;
    fn file_canonicalize(&mut self, name: &Path) -> Result<Option<PathBuf>, Error>;
}

pub struct IncludeReader {}

pub struct IncludeCacher<T: IncludeState> {
    state: T,
    cache_include: HashMap<PathBuf, Rc<Vec<Include<String>>>>,
    cache_canonicalize: HashMap<PathBuf, Option<PathBuf>>,
}

pub enum IncludeBehaviour {
    Clang,
    VisualStudio,
}

pub fn collect_includes<T: IncludeState>(state: &mut T,
                                         source_file: &Path,
                                         include_dir: &[&Path],
                                         combine: &IncludeCombine)
                                         -> Result<HashSet<PathBuf>, Error> {
    let mut queue: Vec<CollectTask> = Vec::new();
    let mut result: HashSet<CollectTask> = HashSet::new();

    let source_canon = try!(source_file.canonicalize());
    queue.push(CollectTask {
        context: combine.combine_context(&Rc::new(Vec::new()), &source_canon),
        source: source_canon,
    });
    loop {
        match queue.pop() {
            Some(task) => {
                if result.insert(task.clone()) {
                    for include in try!(file_include_paths(state, &task.source, &task.context[..], include_dir)).into_iter() {
                        queue.push(CollectTask {
                            context: combine.combine_context(&task.context, &include),
                            source: include,
                        });
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
            cache_canonicalize: HashMap::new(),
            cache_include: HashMap::new(),
        }
    }
}

impl IncludeState for IncludeReader {
    fn file_includes(&mut self, source_file: &Path) -> Result<Rc<Vec<Include<String>>>, Error> {
        println!("> LOAD: {:?}", source_file);
        File::open(Path::new(source_file))
            .and_then(|mut f| {
                let mut v = Vec::new();
                f.read_to_end(&mut v).map(|_| v)
            })
            .and_then(|b| source_includes(&b))
            .map(|v| Rc::new(v))
    }

    fn file_canonicalize(&mut self, path: &Path) -> Result<Option<PathBuf>, Error> {
        match path.is_file() {
            true => path.canonicalize().map(|v| Some(v)),
            false => Ok(None),
        }
    }
}

impl<T: IncludeState> IncludeState for IncludeCacher<T> {
    fn file_includes(&mut self, source_file: &Path) -> Result<Rc<Vec<Include<String>>>, Error> {
        match self.cache_include.entry(source_file.to_path_buf()) {
            Entry::Occupied(entry) => {
                println!("> CACHED: {:?}", source_file);
                Ok(entry.get().clone())
            },
            Entry::Vacant(entry) => self.state.file_includes(source_file).map(|e| entry.insert(e).clone()),
        }
    }

    fn file_canonicalize(&mut self, path: &Path) -> Result<Option<PathBuf>, Error> {
        match self.cache_canonicalize.entry(path.to_path_buf()) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => self.state.file_canonicalize(path).map(|e| entry.insert(e).clone()),
        }
    }
}

fn file_include_paths<T: IncludeState>(state: &mut T,
                                       source_file: &Path,
                                       context_dir: &[PathBuf],
                                       include_dir: &[&Path])
                                       -> Result<Vec<PathBuf>, Error> {
    state.file_includes(source_file).and_then(|v| {
        v.iter()
            .filter_map(|i| match i {
                &Include::Quote(ref name) => {
                    let path = Path::new(name);
                    solve_include_path(state,
                                       path,
                                       context_dir.iter()
                                           .rev()
                                           .map(|p| p.as_path())
                                           .chain(include_dir.iter().map(|p| *p)))
                }
                &Include::Bracket(ref name) => solve_include_path(state, Path::new(name), include_dir.iter().map(|p| *p)),
            })
            .collect()
    })
}

fn solve_include_path<'a, T: IncludeState, I: Iterator<Item = &'a Path>>(state: &mut T,
                                                                         include_path: &Path,
                                                                         dir_iter: I)
                                                                         -> Option<Result<PathBuf, Error>> {
    if include_path.is_absolute() {
        return Some(Ok(include_path.to_path_buf()));
    }
    dir_iter.filter_map(|dir| match state.file_canonicalize(&dir.join(include_path)) {
        Ok(v) => v.map(|v| Ok(v)),
        Err(v) => Some(Err(v)),
    }).next()
}
