// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate clap; // command-line processing
extern crate iron; // web server
extern crate router; // URL mapping
extern crate juniper; // GraphQL
#[macro_use]
extern crate error_chain;

extern crate mercurial;
extern crate mercurial_graphql;

#[cfg(fbcode_build)]
extern crate hgqlserve_build_info; // buck-generated build details

use std::collections::HashMap;
use std::path::{Component, PathBuf};
use std::net::ToSocketAddrs;

use clap::App;

use iron::prelude::*;
use router::Router;
use juniper::EmptyMutation;
use juniper::iron_handlers::{GraphQLHandler, GraphiQLHandler};

use mercurial::RevlogRepo;
use mercurial_graphql::repo::{GQLRepo, RepoCtx};

#[allow(unused)]
#[cfg(fbcode_build)]
use hgqlserve_build_info::BUILDINFO;

mod errors {
    use mercurial;

    error_chain! {
        links {
            Mercurial(mercurial::Error, mercurial::ErrorKind);
        }
    }
}

use errors::*;

// Given an iterator of paths, find the ones that actually appear to be mercurial repos and
// contruct a Repo for each of them.
fn find_repos<I, S>(repos: I) -> HashMap<String, RevlogRepo>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut repomap = HashMap::new();

    for path in repos.into_iter() {
        let path = path.as_ref();
        let mut path = PathBuf::from(path);

        let name = match path.components().rev().take(1).next() {
            Some(Component::Normal(ref n)) => n.to_string_lossy().into_owned(),
            None | Some(_) => {
                println!("path {:?} has no name?", path);
                continue;
            }
        };

        path.push(".hg");

        if !path.exists() || !path.is_dir() {
            println!("path {:?} doesn't exist or isn't a dir", path);
            continue;
        }

        let repo = match RevlogRepo::open(&path) {
            Ok(repo) => repo,
            Err(e) => {
                println!("Failed to open repo {:?}: {}", path, e);
                continue;
            }
        };

        repomap.insert(name, repo);
    }

    repomap
}

fn init_routes(repomap: HashMap<String, RevlogRepo>) -> Router {
    let mut router = Router::new();

    for (name, repo) in repomap {
        let repoctx = RepoCtx::new(repo);
        let handler = GraphQLHandler::new(
            move |_| repoctx.clone(),
            GQLRepo::new(),
            EmptyMutation::new(),
        );

        let path = format!("/{}/gql", name);
        router.post(&path, handler, format!("gql-{}", name));

        let handler = GraphiQLHandler::new(path.as_ref());
        router.get(
            format!("/{}/query", name),
            handler,
            format!("query-{}", name),
        );
    }

    router
}

fn run() -> Result<()> {
    let matches = App::new("hgqlserve")
        .version("0.0.0")
        .about("browse a repo")
        .args_from_usage(concat!(
            "-l, --listen=[LISTEN]  'if/port to listen on'\n",
            "<REPODIR>...           'paths to repo dirs (parent of .hg)'\n"
        ))
        .get_matches();

    let listen = matches.value_of("listen").unwrap_or("localhost:8080");
    let sa = match listen.to_socket_addrs().map(|mut sa| sa.next()) {
        Ok(Some(sa)) => sa,
        Ok(None) => bail!("{} maps to no addresses", listen),
        Err(e) => bail!("Malformed listen address \"{}\": {}", listen, e),
    };

    let repomap = find_repos(matches.values_of("REPODIR").unwrap());

    if repomap.is_empty() {
        bail!("No repos successfully opened");
    }

    for (k, _) in &repomap {
        println!("Repo \"{}\" at http://{}/{}/query", k, listen, k)
    }

    let router = init_routes(repomap);

    match Iron::new(router).http(sa) {
        Ok(_) => println!("OK"),
        Err(e) => println!("Bad! {}", e),
    }

    Ok(())
}

fn main() {
    if let Err(ref e) = run() {
        println!("Failed: {}", e);

        for e in e.iter().skip(1) {
            println!("caused by: {}", e);
        }

        std::process::exit(1);
    }
}
