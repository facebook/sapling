// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

/// Mononoke endpoint for Eden.
///
/// Uses rest API
///
/// # Request examples
/// ```
/// /REPO/cs/HASH/roottreemanifestid - returns root tree manifest node for the HASH
/// ```

extern crate ascii;
extern crate blobrepo;
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate fileheads;
extern crate fileblob;
extern crate filebookmarks;
extern crate futures;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;

use clap::App;
use futures::{Future, Stream};
use hyper::StatusCode;
use hyper::server::{Http, Request, Response, Service};
use mercurial_types::{NodeHash, Repo};
use regex::{Captures, Regex};

mod errors;

use errors::*;

const EXIT_CODE: i32 = 1;

type NameToRepo<BlobRepo> = HashMap<String, Arc<BlobRepo>>;
type UrlParseFunc = fn(Captures) -> Result<ParsedUrl>;

struct Route(Regex, UrlParseFunc);

fn parse_capture<T>(caps: &Captures, index: usize) -> Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: ToString,
    errors::Error: std::convert::From<<T as std::str::FromStr>::Err>,
{

    let s = caps.get(index)
        .expect("incorrect url parsing regex")
        .as_str();
    str::parse::<T>(s).map_err(Error::from)
}

fn parse_root_treemanifest_id_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<NodeHash>(&caps, 2)?;
    Ok(ParsedUrl::RootTreeManifestId(repo, hash))
}

fn parse_tree_content(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<NodeHash>(&caps, 2)?;
    Ok(ParsedUrl::TreeNodeBlob(repo, hash))
}

/// Generic url-handling function
/// Accepts vector of tuples (regex, url handling function)
/// If url matches regex then url handling function is called
fn parse_url(url: &str, routes: &[Route]) -> Result<ParsedUrl> {
    for &Route(ref regex, parse_func) in routes {
        if let Some(caps) = regex.captures(url) {
            return parse_func(caps);
        }
    }
    Err("malformed url".into())
}

enum ParsedUrl {
    RootTreeManifestId(String, NodeHash),
    TreeNodeBlob(String, NodeHash),
}

lazy_static! {
    static ref ROUTES: Vec<Route> = {
        vec![
            // Workaround for https://github.com/rust-lang/rust/issues/20178
            (r"^/(\w+)/cs/(\w+)/roottreemanifestid$", parse_root_treemanifest_id_url as UrlParseFunc),
            (r"^/(\w+)/treenode/(\w+)/$", parse_tree_content as UrlParseFunc),
        ].into_iter().map(|(re, func)| Route(Regex::new(re).expect("bad regex"), func)).collect()
    };
}


#[derive(Serialize)]
struct TreeMetadata {
    hash: NodeHash,
    path: PathBuf,
    #[serde(rename = "type")]
    ty: mercurial_types::Type,
    size: Option<usize>,
}

impl TreeMetadata {
    fn new<E>(size: Option<usize>, entry: Box<mercurial_types::Entry<Error = E>>) -> TreeMetadata
    where
        E: Send + 'static,
    {
        TreeMetadata {
            hash: entry.get_hash().clone(),
            path: entry.get_path().fsencode(false),
            ty: entry.get_type(),
            size,
        }
    }
}

struct EdenServer<BlobRepo> {
    name_to_repo: NameToRepo<BlobRepo>,
}

impl<BlobRepo> EdenServer<BlobRepo>
where
    EdenServer<BlobRepo>: Service,
    BlobRepo: Repo<Error = blobrepo::Error>,
{
    fn new(name_to_repo: NameToRepo<BlobRepo>) -> EdenServer<BlobRepo> {
        EdenServer { name_to_repo }
    }

    fn get_root_tree_manifest_id(
        &self,
        reponame: String,
        hash: &NodeHash,
    ) -> Box<futures::Future<Item = String, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err("unknown repo".into()).boxed();
            }
        };
        repo.get_changeset_by_nodeid(&hash)
            .map(|cs| cs.manifestid().to_string())
            .map_err(Error::from)
            .boxed()
    }

    fn get_tree_content(
        &self,
        reponame: String,
        hash: &NodeHash,
    ) -> Box<futures::Stream<Item = TreeMetadata, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::stream::once(Err("unknown repo".into())).boxed();
            }
        };

        repo.get_manifest_by_nodeid(&hash)
            .map(|manifest| {
                manifest
                    .list()
                    .and_then(|entry| {
                        entry.get_size().map(|size| TreeMetadata::new(size, entry))
                    })
            })
            .flatten_stream()
            .map_err(Error::from)
            .boxed()

    }
}

impl<BlobRepo> Service for EdenServer<BlobRepo>
where
    BlobRepo: Repo<Error = blobrepo::Error>,
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = futures::future::BoxFuture<Self::Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        let mut resp = Response::new();
        let parsed_req = match parse_url(req.uri().path(), &ROUTES) {
            Ok(req) => req,
            Err(err) => {
                resp.set_body(err.to_string());
                resp.set_status(StatusCode::NotFound);
                return futures::future::ok(resp).boxed();
            }
        };

        let result_future = match parsed_req {
            ParsedUrl::RootTreeManifestId(reponame, hash) => {
                self.get_root_tree_manifest_id(reponame, &hash)
            }
            ParsedUrl::TreeNodeBlob(reponame, hash) => {
                self.get_tree_content(reponame, &hash)
                    .map(|metadata| {
                            let err_msg = format!(
                                "failed to get metadata for {}",
                                metadata.path.to_string_lossy()
                            );
                            serde_json::to_value(&metadata).unwrap_or(err_msg.into())
                        })
                    .collect()
                    .map(|entries| {
                        let x: serde_json::Value = entries.into();
                        x.to_string()
                    })
                    .boxed()
            },
        };
        result_future
            .then(|res| {
                match res {
                    Ok(output) => {
                        resp.set_body(output);
                    }
                    Err(e) => {
                        resp.set_body(e.to_string());
                        resp.set_status(StatusCode::NotFound);
                    }
                };
                futures::future::ok(resp)
            })
            .boxed()
    }
}

fn main() {
    let matches = App::new("Mononoke server for Eden")
        .version("0.1")
        .about("Http server that can answers a few Eden requests")
        .args_from_usage(
            "--addr=[ADDRESS] 'Sets a listen address in the form IP:PORT'
             --blobrepo-folder=[FOLDER] 'folder with blobrepo data'
             --reponame=[REPONAME] 'Name of the repository'",
        )
        .get_matches();
    let addr = matches.value_of("addr").unwrap_or("127.0.0.1:3000").parse();
    let blobrepo_folder = matches
        .value_of("blobrepo-folder")
        .expect("Please specify a path to the blobrepo");
    let reponame = matches
        .value_of("reponame")
        .expect("Please specify a reponame")
        .to_string();

    let blobrepo_folder = Path::new(blobrepo_folder);

    let heads_path = blobrepo_folder.join("heads");
    let bookmarks_path = blobrepo_folder.join("bookmarks");
    let blobstore_path = blobrepo_folder.join("blobs");

    let heads = fileheads::FileHeads::<NodeHash>::open(heads_path.clone())
        .expect("couldn't open heads store");
    let bookmarks = filebookmarks::FileBookmarks::<NodeHash>::open(bookmarks_path.clone())
        .expect("counldn't open bookmarks store");
    let blobstore = fileblob::Fileblob::<String, Vec<u8>>::open(blobstore_path.clone())
        .expect("couldn't open blob store");
    let repo = blobrepo::BlobRepo::new(heads, bookmarks, blobstore);

    let mut map = HashMap::new();
    map.insert(reponame, Arc::new(repo));

    let func = move || Ok(EdenServer::new(map.clone()));
    if let Ok(parsed_addr) = addr {
        match Http::new().bind(&parsed_addr, func) {
            Ok(server) => {
                if let Err(error) = server.run() {
                    println!("Error while running service: {}", error);
                    std::process::exit(EXIT_CODE);
                }
            }
            Err(error) => {
                println!("Failed to run server: {}", error);
                std::process::exit(EXIT_CODE);
            }
        }
    } else {
        println!("Failed to parse address");
        std::process::exit(EXIT_CODE);
    };
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_url_parsing() {
        let routes = &ROUTES;
        assert!(parse_url("badurl", &routes).is_err());

        let hash = std::iter::repeat("a").take(40).collect::<String>();
        let correct_url = format!("/repo/cs/{}/roottreemanifestid", hash);
        assert!(parse_url(&correct_url, &routes).is_ok());

        let badhash = std::iter::repeat("x").take(40).collect::<String>();
        let incorrect_url = format!("/repo/cs/{}/roottreemanifestid", badhash);
        assert!(parse_url(&incorrect_url, &routes).is_err());
    }
}
