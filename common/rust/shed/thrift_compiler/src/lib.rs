/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

//! This crate is a wrapper around
//! [fbthrift](https://github.com/facebook/fbthrift)'s compiler. Its main usage
//! is from within [Cargo build
//! scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html) where
//! it might be invoked to generate rust code from thrift files.

use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::mem;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;
use clap::ValueEnum;
use serde::Deserialize;
use which::which;

/// A thrift library 'foo' (say) results in several crates, including 'foo' and
/// 'foo_clients'. We arrange that the thrift compiler wrapper be invoked from
/// the build of all. The behavior of the wrapper is sensitive to the invocation
/// context ('foo' vs 'foo-clients') and this type is used to disambiguate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
pub enum GenContext {
    /// 'types' crate generation context (crate 'foo').
    #[serde(rename = "types", alias = "lib")]
    Types,
    /// 'clients' crate generation context (crate 'foo_clients').
    #[serde(rename = "clients")]
    Clients,
    /// 'services' crate generation context (crate 'foo_services').
    #[serde(rename = "services")]
    Services,
    /// 'mocks' crate generation context (crate 'foo_mocks').
    #[serde(rename = "mocks")]
    Mocks,
}

impl fmt::Display for GenContext {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let t = match self {
            GenContext::Types => "types",
            GenContext::Clients => "clients",
            GenContext::Services => "services",
            GenContext::Mocks => "mocks",
        };
        fmt.write_str(t)
    }
}

/// Builder for thrift compiler wrapper.
pub struct Config {
    thrift_bin: Option<OsString>,
    out_dir: PathBuf,
    gen_context: GenContext,
    base_path: Option<PathBuf>,
    crate_map: Option<PathBuf>,
    types_crate: Option<String>,
    clients_crate: Option<String>,
    options: Option<String>,
    include_srcs: Vec<PathBuf>,
    extra_srcs: Vec<PathBuf>,
}

impl Config {
    /// Return a new configuration with the required parameters set
    pub fn new(
        gen_context: GenContext,
        thrift_bin: Option<OsString>,
        out_dir: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            thrift_bin,
            out_dir,
            gen_context,
            base_path: None,
            crate_map: None,
            types_crate: None,
            clients_crate: None,
            options: None,
            include_srcs: vec![],
            extra_srcs: vec![],
        })
    }

    /// Return a new configuration with parameters computed based on environment variables set by
    /// Cargo's build scrip (OUT_DIR mostly). If THRIFT is in the environment, that will be used as
    /// the Thrift binary. Otherwise, it will be detected in run_compiler.
    pub fn from_env(gen_context: GenContext) -> Result<Self> {
        println!("cargo:rerun-if-env-changed=THRIFT");

        let thrift_bin = env::var_os("THRIFT");
        let out_dir = env::var_os("OUT_DIR")
            .map(PathBuf::from)
            .context("OUT_DIR environment variable must be set")?;

        let crate_map = out_dir.join("cratemap");
        let mut conf = Self::new(gen_context, thrift_bin, out_dir)?;

        if crate_map.is_file() {
            conf.crate_map(crate_map);
        }

        Ok(conf)
    }

    /// Set the base path which is used by the compiler to find thrift files included by input
    /// thrift files. This is also used to find the compiler.
    pub fn base_path(&mut self, value: impl Into<PathBuf>) -> &mut Self {
        self.base_path = Some(value.into());
        self
    }

    /// Set the path to file with crate map definition which is used by the
    /// compiler to infer crate names that will be used in the generated code.
    /// Please refer to code in
    /// fbthrift/thrift/compiler/generate/t_mstch_rust_generator.cc
    /// for the scheme of crate map.
    pub fn crate_map(&mut self, value: impl Into<PathBuf>) -> &mut Self {
        self.crate_map = Some(value.into());
        self
    }

    /// Set the name of the types sub-crate needed by by the thrift-compiler (to
    /// be able to generate things like `use ::foo__types`).
    pub fn types_crate(&mut self, value: impl Into<String>) -> &mut Self {
        self.types_crate = Some(value.into());
        self
    }

    /// Set the name of the clients sub-crate needed by by the thrift-compiler (to
    /// be able to generate things like `use ::foo__clients`).
    pub fn clients_crate(&mut self, value: impl Into<String>) -> &mut Self {
        self.clients_crate = Some(value.into());
        self
    }

    /// Set the options to be passed to `mstch_rust` code generation. Example
    /// options are `serde`.
    pub fn options(&mut self, value: impl Into<String>) -> &mut Self {
        self.options = Some(value.into());
        self
    }

    /// Set extra srcs to be included into the generated crate.
    pub fn include_srcs(&mut self, value: impl IntoIterator<Item = impl AsRef<Path>>) -> &mut Self {
        self.include_srcs
            .extend(value.into_iter().map(|path| path.as_ref().to_owned()));
        self
    }

    /// Set extra srcs to be copied into the generated crate.
    pub fn extra_srcs(&mut self, value: impl IntoIterator<Item = impl AsRef<Path>>) -> &mut Self {
        self.extra_srcs
            .extend(value.into_iter().map(|path| path.as_ref().to_owned()));
        self
    }

    /// Transform a relative path so leading "../"'s are replaced with "_t".
    pub fn remap_to_out_dir(&self, path: &Path) -> PathBuf {
        let mut rem = path;
        let mut parts = PathBuf::new();
        while let Ok(p) = rem.strip_prefix(Component::ParentDir) {
            rem = p;
            parts.push("_t");
        }
        parts.join(rem)
    }

    /// Run the compiler on the input files. As a result a `lib.rs` file will be
    /// generated inside the output dir. The contents of the `lib.rs` can vary
    /// according to the generation context (e.g. for a given thrift library,
    /// 'foo' say, we invoke the generator for the crate 'foo' and for the crate
    /// 'foo-types').
    pub fn run(&self, input_files: impl IntoIterator<Item = impl AsRef<Path>>) -> Result<()> {
        let thrift_bin = self.resolve_thrift_bin()?;

        let input = name_and_path_from_input(input_files)?;
        let out = &self.out_dir;
        fs::create_dir_all(out)?;

        for input in &input {
            println!("cargo:rerun-if-changed={}", input.1.as_ref().display());
        }
        for include_src in &self.include_srcs {
            println!("cargo:rerun-if-changed={}", include_src.to_string_lossy());
            if let GenContext::Types = self.gen_context {
                let out_path = self.remap_to_out_dir(include_src);
                fs::create_dir_all(out.join(out_path.parent().unwrap()))?;
                fs::copy(include_src, out.join(out_path))?;
            }
        }

        for extra_src in &self.extra_srcs {
            println!("cargo:rerun-if-changed={}", extra_src.to_string_lossy());
            if let GenContext::Types = self.gen_context {
                let out_path = self.remap_to_out_dir(extra_src);
                fs::create_dir_all(out.join(out_path.parent().unwrap()))?;
                fs::copy(extra_src, out.join(out_path))?;
            }
        }

        if let [(_name, file)] = &input[..] {
            match self.gen_context {
                GenContext::Types => {
                    self.run_compiler(&thrift_bin, out, file)?;

                    // These files are not of interest here (for now).
                    fs::remove_file(out.join("client.rs"))?;
                    fs::remove_file(out.join("errors.rs"))?;
                    fs::remove_file(out.join("mock.rs"))?;
                    fs::remove_file(out.join("server.rs"))?;

                    // 'types.rs' (together with the remaining files) has the
                    // content we want (but the file needs renaming to
                    // 'lib.rs').
                    fs::rename(out.join("types.rs"), out.join("lib.rs"))?;
                }
                GenContext::Clients => {
                    // The -clients sub-crate.

                    self.run_compiler(&thrift_bin, out, file)?;

                    fs::remove_file(out.join("consts.rs"))?;
                    fs::remove_file(out.join("mock.rs"))?;
                    fs::remove_file(out.join("server.rs"))?;
                    fs::remove_file(out.join("services.rs"))?;
                    fs::remove_file(out.join("types.rs"))?;

                    fs::rename(out.join("client.rs"), out.join("lib.rs"))?;
                }
                GenContext::Services => {
                    // The -services sub-crate.

                    self.run_compiler(&thrift_bin, out, file)?;

                    fs::remove_file(out.join("client.rs"))?;
                    fs::remove_file(out.join("consts.rs"))?;
                    fs::remove_file(out.join("errors.rs"))?;
                    fs::remove_file(out.join("mock.rs"))?;
                    fs::remove_file(out.join("services.rs"))?;
                    fs::remove_file(out.join("types.rs"))?;

                    fs::rename(out.join("server.rs"), out.join("lib.rs"))?;
                }
                GenContext::Mocks => {
                    // The -mocks sub-crate.

                    self.run_compiler(&thrift_bin, out, file)?;

                    fs::remove_file(out.join("client.rs"))?;
                    fs::remove_file(out.join("consts.rs"))?;
                    fs::remove_file(out.join("errors.rs"))?;
                    fs::remove_file(out.join("server.rs"))?;
                    fs::remove_file(out.join("services.rs"))?;
                    fs::remove_file(out.join("types.rs"))?;

                    fs::rename(out.join("mock.rs"), out.join("lib.rs"))?;
                }
            }
        } else {
            match self.gen_context {
                GenContext::Types => {
                    for (name, file) in &input {
                        let submod = out.join(name);
                        fs::create_dir_all(&submod)?;
                        self.run_compiler(&thrift_bin, &submod, file)?;

                        // These files are not of interest here.
                        fs::remove_file(submod.join("client.rs"))?;
                        fs::remove_file(submod.join("mock.rs"))?;
                        fs::remove_file(submod.join("server.rs"))?;

                        // 'types.rs' (together with the remaining files) has the
                        // content we want (but the file needs renaming to
                        // 'mod.rs').
                        fs::rename(submod.join("types.rs"), submod.join("mod.rs"))?;
                    }
                }
                GenContext::Clients => {
                    // The -clients sub-crate.

                    for (name, file) in &input {
                        let submod = out.join(name);
                        fs::create_dir_all(&submod)?;
                        self.run_compiler(&thrift_bin, &submod, file)?;

                        fs::remove_file(submod.join("consts.rs"))?;
                        fs::remove_file(submod.join("errors.rs"))?;
                        fs::remove_file(submod.join("mock.rs"))?;
                        fs::remove_file(submod.join("server.rs"))?;
                        fs::remove_file(submod.join("services.rs"))?;
                        fs::remove_file(submod.join("types.rs"))?;

                        fs::rename(submod.join("client.rs"), submod.join("mod.rs"))?;
                    }
                }
                GenContext::Services => {
                    // The -services sub-crate.

                    for (name, file) in &input {
                        let submod = out.join(name);
                        fs::create_dir_all(&submod)?;
                        self.run_compiler(&thrift_bin, &submod, file)?;

                        fs::remove_file(submod.join("client.rs"))?;
                        fs::remove_file(submod.join("consts.rs"))?;
                        fs::remove_file(submod.join("errors.rs"))?;
                        fs::remove_file(submod.join("mock.rs"))?;
                        fs::remove_file(submod.join("services.rs"))?;
                        fs::remove_file(submod.join("types.rs"))?;

                        fs::rename(submod.join("server.rs"), submod.join("mod.rs"))?;
                    }
                }
                GenContext::Mocks => {
                    // The -mocks sub-crate.

                    for (name, file) in &input {
                        let submod = out.join(name);
                        fs::create_dir_all(&submod)?;
                        self.run_compiler(&thrift_bin, &submod, file)?;

                        fs::remove_file(submod.join("client.rs"))?;
                        fs::remove_file(submod.join("consts.rs"))?;
                        fs::remove_file(submod.join("errors.rs"))?;
                        fs::remove_file(submod.join("server.rs"))?;
                        fs::remove_file(submod.join("services.rs"))?;
                        fs::remove_file(submod.join("types.rs"))?;

                        fs::rename(submod.join("mock.rs"), submod.join("mod.rs"))?;
                    }
                }
            }

            let lib = format!(
                "{}\n",
                input
                    .iter()
                    .map(|(name, _file)| format!("pub mod {};", name.to_string_lossy()))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            fs::write(out.join("lib.rs"), lib)?;
        }

        Ok(())
    }

    fn resolve_thrift_bin(&self) -> Result<PathBuf> {
        // Get raw location
        let thrift_bin = if let Some(bin) = self.thrift_bin.as_ref() {
            Cow::Borrowed(bin)
        } else {
            Cow::Owned(self.infer_thrift_binary())
        };
        // Resolve based on PATH if needed
        let thrift_bin_path = Path::new(&*thrift_bin);
        let mut thrift_bin = if thrift_bin_path.components().count() == 1 {
            println!("cargo:rerun-if-env-changed=PATH");
            which(thrift_bin_path).with_context(|| {
                format!(
                    "Failed to resolve thrift binary `{}` to an absolute path",
                    thrift_bin.to_string_lossy()
                )
            })?
        } else {
            thrift_bin_path.to_owned()
        };
        println!("cargo:rerun-if-changed={}", thrift_bin.to_string_lossy());
        // Canonicalize so that it refers to the same path when run as a
        // subprocess in a different current directory.
        if self.base_path.is_some() {
            thrift_bin = dunce::canonicalize(thrift_bin)?;
        }
        Ok(thrift_bin)
    }

    fn infer_thrift_binary(&self) -> OsString {
        if let Some(base) = self.base_path.as_ref() {
            let mut candidate = base.clone();
            candidate.push("thrift/facebook/rpm/thrift1");
            #[cfg(windows)]
            candidate.set_extension("exe");
            if Path::new(&candidate).exists() {
                return candidate.into_os_string();
            }
        }

        "thrift1".into()
    }

    fn run_compiler(
        &self,
        thrift_bin: &Path,
        out: impl AsRef<Path>,
        input: impl AsRef<Path>,
    ) -> Result<()> {
        let mut mstch_rust_args = OsString::from("mstch_rust");
        let mut separator = {
            let mut separator = ":";
            move || mem::replace(&mut separator, ",")
        };
        if let Some(crate_map) = &self.crate_map {
            let crate_map = dunce::canonicalize(crate_map)?;
            mstch_rust_args.push(separator());
            mstch_rust_args.push("cratemap=");
            mstch_rust_args.push(crate_map);
        }
        if let Some(types_crate) = &self.types_crate {
            mstch_rust_args.push(separator());
            mstch_rust_args.push("types_crate=");
            mstch_rust_args.push(types_crate);
        }
        if let Some(clients_crate) = &self.clients_crate {
            mstch_rust_args.push(separator());
            mstch_rust_args.push("clients_crate=");
            mstch_rust_args.push(clients_crate);
        }
        if let Some(include_srcs_arg_name) = match self.gen_context {
            _ if self.include_srcs.is_empty() => None,
            GenContext::Types => Some("types_include_srcs="),
            GenContext::Clients => Some("clients_include_srcs="),
            GenContext::Services => Some("services_include_srcs="),
            GenContext::Mocks => None,
        } {
            mstch_rust_args.push(separator());
            mstch_rust_args.push(include_srcs_arg_name);
            for (i, src) in self.include_srcs.iter().enumerate() {
                if i != 0 {
                    mstch_rust_args.push(":");
                }
                mstch_rust_args.push(self.remap_to_out_dir(src));
            }
        }
        if let Some(options) = &self.options {
            mstch_rust_args.push(separator());
            mstch_rust_args.push(options);
        }

        let mut cmd = Command::new(thrift_bin);
        cmd.arg("--gen").arg(mstch_rust_args);

        let out = dunce::canonicalize(out)?;
        cmd.arg("--out").arg(out);

        if let Some(base_path) = &self.base_path {
            cmd.current_dir(base_path);
            cmd.arg("-I");
            cmd.arg(".");
            cmd.arg(relative_path(base_path, input)?);
        } else {
            cmd.arg(input.as_ref());
        }

        let output = cmd.output().with_context(|| {
            format!(
                "Failed to run thrift compiler. Is '{}' executable?",
                thrift_bin.to_string_lossy()
            )
        })?;
        ensure!(
            output.status.success(),
            format!(
                "Command '{:#?}' failed! Stdout:\n{}\nStderr:\n{}",
                cmd,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            )
        );

        Ok(())
    }
}

fn name_and_path_from_input<T: AsRef<Path>>(
    input_files: impl IntoIterator<Item = T>,
) -> Result<Vec<(OsString, T)>> {
    input_files
        .into_iter()
        .map(|file| {
            Ok((
                file.as_ref()
                    .file_stem()
                    .ok_or_else(|| {
                        anyhow!(
                            "Failed to get file_stem from path {}",
                            file.as_ref().display()
                        )
                    })?
                    .to_owned(),
                file,
            ))
        })
        .collect()
}

fn relative_path(base: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<PathBuf> {
    let base = dunce::canonicalize(base)?;
    let destination = dunce::canonicalize(destination)?;

    let mut common_prefix = base.as_path();
    let mut upward = PathBuf::new();
    loop {
        if let Ok(relative) = destination.strip_prefix(common_prefix) {
            return Ok(upward.join(relative));
        } else {
            upward.push("..");
            common_prefix = common_prefix
                .parent()
                .with_context(|| {
                    format!(
                        "cannot express {} relative to {}",
                        destination.display(),
                        base.display(),
                    )
                })
                .unwrap();
        }
    }
}
