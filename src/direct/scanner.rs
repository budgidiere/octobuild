use std::collections::HashSet;
use std::path::{PathBuf, Path};
use std::io::{Error, Read};
use std::fs::File;
use std::rc::Rc;

use ::filter::includes::{Include, source_includes};

#[derive(Clone)]
#[derive(Hash)]
#[derive(Eq)]
#[derive(PartialEq)]
struct CollectTask {
    context: Rc<Vec<PathBuf>>,
    source: PathBuf,
}

pub fn collect_includes(source_file: &Path, include_dir: &[&Path]) -> Result<HashSet<PathBuf>, Error> {
    let mut queue: Vec<CollectTask> = Vec::new();
    let mut result: HashSet<CollectTask> = HashSet::new();

    let source_canon = try!(source_file.canonicalize());
    queue.push(CollectTask { context: combine_context(&Rc::new(Vec::new()), &source_canon), source: source_canon });
    loop {
        match queue.pop() {
            Some(task) => {
                if result.insert(task.clone()) {
                    for include in try!(file_include_paths(&task.source, &[task.source.parent().unwrap()], include_dir)).into_iter() {
                        queue.push(CollectTask { context: combine_context(&task.context, &include), source: include });
                    }
                }
            },
            None => { break; }
        }
    }
    Ok(result.into_iter().map(|t| t.source).collect())
}

fn combine_context(context: &Rc<Vec<PathBuf>>, source_path: &Path) -> Rc<Vec<PathBuf>> {
    Rc::new(source_path.parent().map(|p| p.to_path_buf()).into_iter().collect())
}

fn file_includes(source_file: &Path) -> Result<Vec<Include<String>>, Error> {
    File::open(Path::new(source_file))
        .and_then(|mut f| {
            let mut v = Vec::new();
            f.read_to_end(&mut v).map(|_| v)
        })
        .and_then(|b| source_includes(&b))
}

fn file_include_paths(source_file: &Path, context_dir: &[&Path], include_dir: &[&Path]) -> Result<Vec<PathBuf>, Error> {
    file_includes(source_file)
        .and_then(|v| v.into_iter().filter_map(|i|
            match i {
                Include::Quote(path) => solve_include_path(Path::new(&path), context_dir).or_else(|| solve_include_path(Path::new(&path), include_dir)),
                Include::Bracket(path) => solve_include_path(Path::new(&path), include_dir),
            }).collect())
}

fn solve_include_path(include_path: &Path, search_dir: &[&Path]) -> Option<Result<PathBuf, Error>> {
    if include_path.is_absolute() {
        return Some(Ok(include_path.to_path_buf()));
    }
    for dir in search_dir.iter() {
        let path = dir.join(include_path);
        if path.is_file() {
            return Some(path.canonicalize());
        }
    }
    None
}