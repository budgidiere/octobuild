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

pub enum IncludeBehaviour {
    Clang,
    VisualStudio,
}

pub fn collect_includes(source_file: &Path,
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
                    for include in try!(file_include_paths(&task.source, &task.context[..], include_dir)).into_iter() {
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

fn file_includes(source_file: &Path) -> Result<Vec<Include<String>>, Error> {
    File::open(Path::new(source_file))
        .and_then(|mut f| {
            let mut v = Vec::new();
            f.read_to_end(&mut v).map(|_| v)
        })
        .and_then(|b| source_includes(&b))
}

fn file_include_paths(source_file: &Path,
                      context_dir: &[PathBuf],
                      include_dir: &[&Path])
                      -> Result<Vec<PathBuf>, Error> {
    println!("> {:?} ({:?})", source_file, context_dir);
    file_includes(source_file).and_then(|v| {
        v.into_iter()
            .filter_map(|i| match i {
                Include::Quote(name) => {
                    let path = Path::new(&name);
                    solve_include_path(path,
                                       context_dir.iter()
                                           .rev()
                                           .map(|p| p.as_path())
                                           .chain(include_dir.iter().map(|p| *p)))
                }
                Include::Bracket(name) => solve_include_path(Path::new(&name), include_dir.iter().map(|p| *p)),
            })
            .collect()
    })
}

fn solve_include_path<'a, I: Iterator<Item = &'a Path>>(include_path: &Path,
                                                        dir_iter: I)
                                                        -> Option<Result<PathBuf, Error>> {
    if include_path.is_absolute() {
        return Some(Ok(include_path.to_path_buf()));
    }
    for dir in dir_iter {
        let path = dir.join(include_path);
        if path.is_file() {
            return Some(path.canonicalize());
        }
    }
    None
}
