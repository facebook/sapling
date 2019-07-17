// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::string::String;

use clap::{App, Arg, ArgMatches, SubCommand};
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use apiserver_client::MononokeAPIClient;

fn cat(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let revision = matches.value_of("revision").expect("must provide revision");
    let path = matches.value_of("path").expect("must provide path");
    let bookmark = matches.is_present("bookmark");

    client
        .get_raw(revision.to_string(), path.to_string(), bookmark)
        .map_err(|e| eprintln!("error: {}", e))
        .and_then(|r| String::from_utf8(r).map_err(|_| eprintln!("error: utf8 conversion failed.")))
        .map(|res| {
            println!("{}", res);
        })
        .boxify()
}

fn get_changeset(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let revision = matches
        .value_of("revision")
        .expect("must provide changeset");

    client
        .get_changeset(revision.to_string())
        .and_then(|r| {
            Ok(serde_json::to_string(&r).unwrap_or("Error converting request to json".to_string()))
        })
        .map_err(|e| eprintln!("error: {}", e))
        .map(|res| println!("{}", res))
        .boxify()
}

fn get_branches(client: MononokeAPIClient) -> BoxFuture<(), ()> {
    client
        .get_branches()
        .and_then(|r| {
            Ok(serde_json::to_string(&r).unwrap_or("Error converting request to json".to_string()))
        })
        .map_err(|e| eprintln!("error: {}", e))
        .map(|res| println!("{}", res))
        .boxify()
}

fn list_directory(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let revision = matches.value_of("revision").expect("must provide revision");
    let path = matches.value_of("path").expect("must provide path");

    client
        .list_directory(revision.to_string(), path.to_string())
        .and_then(|r| {
            Ok(serde_json::to_string(&r).unwrap_or("Error converting request to json".to_string()))
        })
        .map_err(|e| eprintln!("error: {}", e))
        .map(|res| {
            println!("{}", res);
        })
        .boxify()
}

fn is_ancestor(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let ancestor = matches.value_of("ancestor").expect("must provide ancestor");
    let descendant = matches
        .value_of("descendant")
        .expect("must provide descendant");

    client
        .is_ancestor(ancestor.to_string(), descendant.to_string())
        .and_then(|r| {
            Ok(serde_json::to_string(&r).unwrap_or("Error converting request to json".to_string()))
        })
        .map_err(|e| eprintln!("error: {}", e))
        .map(|res| {
            println!("{}", res);
        })
        .boxify()
}

fn get_blob(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let hash = matches
        .value_of("hash")
        .expect("must provide hash of the blob");

    client
        .get_blob(hash.to_string())
        .map_err(|e| eprintln!("error: {}", e))
        .map(|r| unsafe { println!("{}", String::from_utf8_unchecked(r.content)) })
        .boxify()
}

fn get_tree(client: MononokeAPIClient, matches: &ArgMatches<'_>) -> BoxFuture<(), ()> {
    let hash = matches
        .value_of("hash")
        .expect("must provide hash of the tree");

    client
        .get_tree(hash.to_string())
        .and_then(|r| {
            Ok(serde_json::to_string(&r).unwrap_or("Error converting request to json".to_string()))
        })
        .map_err(|e| eprintln!("error: {}", e))
        .map(|res| {
            println!("{}", res);
        })
        .boxify()
}

fn main() -> Result<(), ()> {
    let matches = App::new("Mononoke API Server Thrift client")
        .about("Send requests to Mononoke API Server thrift port")
        .arg(
            Arg::with_name("tier")
                .short("t")
                .long("tier")
                .value_name("TIER")
                .help("tier name")
                .default_value("mononoke-apiserver-thrift"),
        )
        .arg(
            Arg::with_name("repo")
                .short("r")
                .long("repo")
                .value_name("NAME")
                .help("repository name (e.g. fbsource)")
                .default_value("fbsource"),
        )
        .subcommand(
            SubCommand::with_name("cat")
                .about("retrieve file content")
                .arg(
                    Arg::with_name("revision")
                        .short("c")
                        .long("revision")
                        .value_name("HASH")
                        .help("hash/bookmark of the revision you want to query")
                        .required(true),
                )
                .arg(
                    Arg::with_name("path")
                        .short("p")
                        .long("path")
                        .value_name("PATH")
                        .help("path to the file you want to get")
                        .required(true),
                )
                .arg(
                    Arg::with_name("bookmark")
                        .short("b")
                        .long("bookmark")
                        .help("Revision is a bookmark"),
                ),
        )
        .subcommand(
            SubCommand::with_name("get_changeset")
                .about("get information about a changeset")
                .arg(
                    Arg::with_name("revision")
                        .short("c")
                        .long("revision")
                        .value_name("HASH")
                        .help("hash/bookmark of the revision you want to query")
                        .required(true),
                ),
        )
        .subcommand(SubCommand::with_name("get_branches").about("get all branches"))
        .subcommand(
            SubCommand::with_name("list_directory")
                .about("list all files in a directory")
                .arg(
                    Arg::with_name("revision")
                        .short("c")
                        .long("revision")
                        .value_name("HASH")
                        .help("hash/bookmark of the revision you want to query")
                        .required(true),
                )
                .arg(
                    Arg::with_name("path")
                        .short("p")
                        .long("path")
                        .value_name("PATH")
                        .help("path to the directory you want to list")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("ancestor")
                .about("Check whether a commit is descendant of a given ancestor")
                .arg(
                    Arg::with_name("ancestor")
                        .short("a")
                        .long("ancestor")
                        .value_name("HASH")
                        .help("hash/bookmark of the ancestor")
                        .required(true),
                )
                .arg(
                    Arg::with_name("descendant")
                        .short("d")
                        .long("descendant")
                        .value_name("HASH")
                        .help("hash/bookmark of the descendant")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("get_blob")
                .about("get blob content with hash")
                .arg(
                    Arg::with_name("hash")
                        .short("H")
                        .long("hash")
                        .value_name("HASH")
                        .help("hash of the blob")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("get_tree")
                .about("get blob content with hash")
                .arg(
                    Arg::with_name("hash")
                        .short("H")
                        .long("hash")
                        .value_name("HASH")
                        .help("hash of the tree")
                        .required(true),
                ),
        )
        .get_matches();

    let tier = matches.value_of("tier").expect("must provide tier name");
    let repo = matches.value_of("repo").expect("must provide repo name");

    let client =
        MononokeAPIClient::new_with_tier_repo(tier, repo).map_err(|e| eprintln!("error: {}", e))?;

    let future = if let Some(matches) = matches.subcommand_matches("cat") {
        cat(client, matches)
    } else if let Some(matches) = matches.subcommand_matches("get_changeset") {
        get_changeset(client, matches)
    } else if let Some(_) = matches.subcommand_matches("get_branches") {
        get_branches(client)
    } else if let Some(matches) = matches.subcommand_matches("list_directory") {
        list_directory(client, matches)
    } else if let Some(matches) = matches.subcommand_matches("ancestor") {
        is_ancestor(client, matches)
    } else if let Some(matches) = matches.subcommand_matches("get_blob") {
        get_blob(client, matches)
    } else if let Some(matches) = matches.subcommand_matches("get_tree") {
        get_tree(client, matches)
    } else {
        Ok(()).into_future().boxify()
    };

    tokio::run(future);

    Ok(())
}
