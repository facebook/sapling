// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
extern crate fileheads;
extern crate fileblob;
extern crate filebookmarks;
extern crate futures;
extern crate hyper;
extern crate mercurial_types;
extern crate url;

use ascii::AsciiStr;
use clap::App;
use futures::Future;
use hyper::StatusCode;
use hyper::server::{Http, Request, Response, Service};
use mercurial_types::{NodeHash, Repo};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

const EXIT_CODE: i32 = 1;

fn parse_hash(changeset: &str) -> Result<NodeHash, String> {
    let asciichangeset = AsciiStr::from_ascii(changeset).map_err(|e| e.to_string())?;
    let node_id = NodeHash::from_ascii_str(asciichangeset)
        .map_err(|e| e.kind().description().to_string())?;

    return Result::Ok(node_id);
}

fn parse_url<'a, BlobRepo>(
    segments: &Vec<&str>,
    name_to_repo: &'a NameToRepo<BlobRepo>,
) -> ParsedUrl<'a>
where
    BlobRepo: Repo<Error = blobrepo::Error>,
{
    if segments.len() == 5 && segments[2] == "cs" && segments[4] == "roottreemanifestid" {
        let reponame = segments[1];
        let repo_hash = name_to_repo
            .get(reponame)
            .ok_or("unknown repo".to_string())
            .and_then(|repo| {
                let hash = segments[3];
                match parse_hash(hash) {
                    Ok(hash) => Ok((repo, hash)),
                    Err(err) => Err(err.to_string()),
                }
            });

        match repo_hash {
            Ok((repo, hash)) => ParsedUrl::RootTreeManifestId(repo.deref(), hash),
            Err(err) => ParsedUrl::Err(err.to_string()),
        }
    } else {
        let malformed_url_msg = "malformed url: expected /REPONAME/cs/HASH/roottreemanifestid";
        ParsedUrl::Err(malformed_url_msg.to_string())
    }
}

enum ParsedUrl<'a> {
    RootTreeManifestId(&'a Repo<Error = blobrepo::Error>, NodeHash),
    Err(String),
}

struct EdenServer<BlobRepo> {
    name_to_repo: NameToRepo<BlobRepo>,
}

type NameToRepo<BlobRepo> = HashMap<String, Arc<BlobRepo>>;

impl<BlobRepo> EdenServer<BlobRepo>
where
    EdenServer<BlobRepo>: Service,
{
    fn new(name_to_repo: NameToRepo<BlobRepo>) -> EdenServer<BlobRepo> {
        EdenServer::<BlobRepo> { name_to_repo: name_to_repo }
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
        let segments: Vec<_> = req.uri().path().split('/').collect();
        let mut resp = Response::new();
        match parse_url(&segments, &self.name_to_repo) {
            ParsedUrl::RootTreeManifestId(repo, hash) => {
                repo.get_changeset_by_nodeid(&hash)
                    .then(|res| {
                        match res {
                            Ok(cs) => {
                                resp.set_body(cs.manifestid().to_string());
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
            ParsedUrl::Err(err) => {
                resp.set_body(err);
                resp.set_status(StatusCode::NotFound);
                futures::future::ok(resp).boxed()
            }
        }
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
    let bookmarks = filebookmarks::FileBookmarks::<String>::open(bookmarks_path.clone())
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
