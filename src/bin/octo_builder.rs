extern crate octobuild;
extern crate capnp;
extern crate daemon;
extern crate router;
extern crate fern;
extern crate hyper;
extern crate rustc_serialize;
extern crate tempdir;
#[macro_use]
extern crate log;

use octobuild::compiler::*;
use octobuild::cluster::builder::CompileRequest;
use octobuild::cluster::common::{BuilderInfo, BuilderInfoUpdate, RPC_BUILDER_UPDATE};
use octobuild::builder_capnp;
use octobuild::version;
use octobuild::vs::compiler::VsCompiler;
use octobuild::clang::compiler::ClangCompiler;
use octobuild::io::memstream::MemStream;
use daemon::State;
use daemon::Daemon;
use daemon::DaemonRunner;
use hyper::{Client, Url};
use rustc_serialize::json;
use tempdir::TempDir;
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::io::{BufReader, Read, Write};
use std::iter::FromIterator;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Receiver;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::str::FromStr;
use std::time::Duration;
use std::thread;
use std::thread::JoinHandle;

use capnp::serialize_packed;
use capnp::message;

struct BuilderService {
    done: Arc<AtomicBool>,
    listener: Option<TcpListener>,
    accepter: Option<JoinHandle<()>>,
    anoncer: Option<JoinHandle<()>>,
}

struct BuilderState {
    name: String,
    endpoint: SocketAddr,
    toolchains: HashMap<String, Arc<Toolchain>>,
}

impl BuilderService {
    fn new() -> Self {
        let addr: SocketAddr = FromStr::from_str("127.0.0.1:0")
            .ok()
            .expect("Failed to parse host:port string");
        let listener = TcpListener::bind(&addr).ok().expect("Failed to bind address");

        let state = Arc::new(BuilderState {
            name: get_name(),
            endpoint: listener.local_addr().unwrap(),
            toolchains: BuilderService::discovery_toolchains(),
        });

        info!("Found toolchains:");
        for toolchain in state.toolchain_names().iter() {
            info!("- {}", toolchain);
        }

        let done = Arc::new(AtomicBool::new(false));
        BuilderService {
            accepter: Some(BuilderService::thread_accepter(state.clone(), listener.try_clone().unwrap())),
            anoncer: Some(BuilderService::thread_anoncer(state.clone(), done.clone())),
            done: done,
            listener: Some(listener),
        }
    }

    fn thread_accepter(state: Arc<BuilderState>, listener: TcpListener) -> JoinHandle<()> {
        thread::spawn(move || {
            // accept connections and process them, spawning a new thread for each one
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let state_copy = state.clone();
                        thread::spawn(move || {
                            // connection succeeded
                            BuilderService::handle_client(state_copy, stream)
                        });
                    }
                    Err(e) => {
                        // connection failed
                    }
                }
            }
        })
    }

    fn thread_anoncer(state: Arc<BuilderState>, done: Arc<AtomicBool>) -> JoinHandle<()> {
        thread::spawn(move || {
            let info = BuilderInfoUpdate::new(BuilderInfo {
                name: state.name.clone(),
                version: version::short_version(),
                endpoint: state.endpoint.to_string(),
                toolchains: state.toolchain_names(),
            });

            let client = Client::new();
            while !done.load(Ordering::Relaxed) {
                match client.post(Url::parse("http://localhost:3000")
                        .unwrap()
                        .join(RPC_BUILDER_UPDATE)
                        .unwrap())
                    .body(&json::encode(&info).unwrap())
                    .send() {
                    Ok(_) => {}
                    Err(e) => {
                        info!("Builder: can't send info to coordinator: {}",
                              e.description());
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        })
    }

    fn handle_client(state: Arc<BuilderState>, mut stream: TcpStream) -> io::Result<()> {
        {
            let mut buf = BufReader::new(try!(stream.try_clone()));
            // Receive compilation request.
            {
                let request = CompileRequest::read(&mut buf, ::capnp::message::ReaderOptions::new()).unwrap();

                println!("{:?}", request);
                let compile_step: CompileStep = CompileStep {
                    output_object: None,
                    output_precompiled: None,
                    input_precompiled: None,
                    args: request.args,
                    preprocessed: MemStream::from(request.preprocessed),
                };

                let toolchain: Arc<Toolchain> = state.toolchains.get(&request.toolchain).unwrap().clone();
                let output_info: OutputInfo = toolchain.compile_step(compile_step).unwrap();

                // Send compilation request.
                let mut builder = message::Builder::new_default();
                {
                    // Toolchain.
                    let mut response = builder.init_root::<builder_capnp::compile_response::Builder>();
                    let mut success = response.borrow().init_success();
                    match output_info.status {
                        Some(status) => success.set_status(status),
                        None => {}
                    }
                }
                serialize_packed::write_message(&mut stream, &mut builder);


            }
        }
        try!(stream.write("Hello!!!\n".as_bytes()));
        try!(stream.flush());
        try!(stream.shutdown(Shutdown::Write));
        let _ = stream.read(&mut [0; 1]);
        Ok(())
    }

    fn discovery_toolchains() -> HashMap<String, Arc<Toolchain>> {
        let temp_dir = TempDir::new("octobuild").ok().expect("Can't create temporary directory");
        let compilers: Vec<Box<Compiler>> = vec!(
            Box::new(VsCompiler::new(temp_dir.path())),
            Box::new(ClangCompiler::new()),
        );
        HashMap::from_iter(compilers.iter()
            .flat_map(|compiler| compiler.discovery_toolchains())
            .filter_map(|toolchain| toolchain.identifier().map(|name| (name, toolchain))))
    }
}

impl BuilderState {
    fn toolchain_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.toolchains.keys().map(|s| s.clone()).collect();
        names.sort();
        names
    }
}

impl Drop for BuilderService {
    fn drop(&mut self) {
        println!("drop begin");
        self.done.store(true, Ordering::Relaxed);
        self.listener.take();

        match self.anoncer.take() {
            Some(t) => {
                t.join().unwrap();
            }
            None => {}
        }
        match self.accepter.take() {
            Some(t) => {
                t.join().unwrap();
            }
            None => {}
        }
        println!("drop end");
    }
}

fn get_name() -> String {
    octobuild::hostname::get_host_name().unwrap()
}

fn main() {
    let daemon = Daemon { name: "octobuild_Builder".to_string() };

    daemon.run(move |rx: Receiver<State>| {
            octobuild::utils::init_logger();

            info!("Builder started.");
            let mut builder = None;
            for signal in rx.iter() {
                match signal {
                    State::Start => {
                        info!("Builder: Starting");
                        builder = Some(BuilderService::new());
                        info!("Builder: Ready");
                    }
                    State::Reload => {
                        info!("Builder: Reload");
                    }
                    State::Stop => {
                        info!("Builder: Stoping");
                        builder.take();
                        info!("Builder: Stoped");
                    }
                };
            }
            info!("Builder shutdowned.");
        })
        .unwrap();
}