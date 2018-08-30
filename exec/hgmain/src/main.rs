extern crate encoding;
extern crate libc;
extern crate python27_sys;

mod python;
use python::{py_main, py_set_python_home};
use std::env;
use std::path::{Path, PathBuf};

/// A default name of the python script that this Rust binary will try to
/// load when it decides to pass control to Python
const HGPYENTRYPOINT: &str = "entrypoint.py";

struct HgPython {
    installation_root: PathBuf,
}

impl HgPython {
    pub fn new() -> HgPython {
        let exe_path = env::current_exe().expect("failed to call current_exe");
        let installation_root = exe_path.parent().unwrap();
        Self::setup(&installation_root);

        HgPython {
            installation_root: installation_root.to_path_buf(),
        }
    }

    fn setup(installation_root: &Path) {
        if cfg!(target_os = "windows") {
            py_set_python_home(installation_root.join("hg-python"));
        }
    }

    fn find_hg_py_entry_point(&self) -> PathBuf {
        let mut candidates: Vec<PathBuf> = vec![];

        // Pri 0: entry point from the environment is a file, not a dir
        if let Ok(env_entry_point) = env::var("HGPYENTRYPOINT") {
            candidates.push(PathBuf::from(env_entry_point));
        }

        // Pri 1: the dir where the binary lives
        candidates.push(
            self.installation_root
                .join("mercurial")
                .join(HGPYENTRYPOINT),
        );

        // TODO: Pri 2: read the config file, which may specify the entrypoint location

        // Pri 3: a list of compile-time provided paths to check
        // Note that HGPYENTRYPOINTSEARCHPATH is in a PATH format and each item is
        // expected to end in mercurial/
        if let Some(compile_time_locations) = option_env!("HGPYENTRYPOINTSEARCHPATH") {
            for path in env::split_paths(compile_time_locations) {
                candidates.push(path.join(HGPYENTRYPOINT));
            }
        }

        // Pri 4: a list of source-level hardcoded paths to check
        candidates.push(
            PathBuf::from("/usr/lib64/python2.7/site-packages/mercurial/").join(HGPYENTRYPOINT),
        );
        candidates.push(
            PathBuf::from("/usr/lib/python2.7/site-packages/mercurial/").join(HGPYENTRYPOINT),
        );

        for candidate in candidates.iter() {
            if candidate.exists() {
                return candidate.clone();
            }
        }
        panic!("could not find {} in {:?}", HGPYENTRYPOINT, candidates);
    }

    fn args_to_local_bytes() -> Vec<Vec<u8>> {
        env::args_os()
            .map(|x| encoding::osstring_to_local_bytes(&x).unwrap().to_vec())
            .collect()
    }

    pub fn run_main(&self) {
        let hgpyentrypoint = encoding::path_to_local_bytes(&self.find_hg_py_entry_point())
            .unwrap()
            .to_vec();
        let mut args: Vec<Vec<u8>> = Self::args_to_local_bytes();
        args.insert(1, hgpyentrypoint);
        let code = py_main(args);
        std::process::exit(code);
    }
}

fn main() {
    let hgpython = HgPython::new();
    hgpython.run_main();
}
