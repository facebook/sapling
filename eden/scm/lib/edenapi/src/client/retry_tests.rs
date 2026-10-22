/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use edenapi_types::BlameData;
use edenapi_types::CheckPathPermissionData;
use edenapi_types::HgChangesetContent;
use edenapi_types::PathHistoryEntries;
use edenapi_types::WireHistoryEntry;
use edenapi_types::commit::HgInfo;
use edenapi_types::legacy::Metadata;
use edenapi_types::legacy::StreamingChangelogBlob;
use edenapi_types::legacy::StreamingChangelogData;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use types::Parents;

use super::*;
use crate::builder::HttpClientBuilder;

struct Reply {
    status: u16,
    body: Vec<u8>,
    truncated: bool,
}

impl Reply {
    fn status(status: u16) -> Self {
        Self {
            status,
            body: Vec::new(),
            truncated: false,
        }
    }

    fn body(body: Vec<u8>, truncated: bool) -> Self {
        Self {
            status: 200,
            body,
            truncated,
        }
    }

    fn incomplete_cbor(mut body: Vec<u8>) -> Self {
        // A complete HTTP body lets the decoder emit the prefix before reporting partial CBOR.
        body.push(0xa1); // Map with one missing key/value pair.
        Self::body(body, false)
    }
}

async fn server(replies: Vec<Reply>, retries: usize) -> (Client, JoinHandle<Vec<Vec<u8>>>) {
    server_with_retries(replies, retries, true).await
}

async fn server_with_retries(
    replies: Vec<Reply>,
    retries: usize,
    stream_retries: bool,
) -> (Client, JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut socket, _) = timeout(Duration::from_secs(10), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(socket.read_u8().await.unwrap());
            }
            let headers = String::from_utf8(headers).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.unwrap();
            requests.push(body);

            let length = reply.body.len() + usize::from(reply.truncated);
            let headers = format!(
                "HTTP/1.1 {} Test\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n",
                reply.status
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
            socket.write_all(&reply.body).await.unwrap();
            socket.shutdown().await.unwrap();
        }
        requests
    });
    let client = HttpClientBuilder::new()
        .repo_name("test")
        .server_url(format!("http://{address}").parse().unwrap())
        .max_retry_per_request(retries)
        .stream_retries(stream_retries)
        .max_history_per_batch(Some(1000))
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    (client, task)
}

#[tokio::test]
async fn history_retries_status_and_transport_errors() {
    for enabled in [true, false] {
        for length in [Some(1), None] {
            for failure in [Reply::status(503), Reply::body(Vec::new(), true)] {
                let mut replies = vec![failure];
                if enabled {
                    replies.push(Reply::status(200));
                }
                let (client, requests) = server_with_retries(replies, 1, enabled).await;
                let result = client
                    .history(vec![Key::default()], length)
                    .await
                    .unwrap()
                    .flatten()
                    .await;
                assert_eq!(result.is_ok(), enabled);
                let requests = requests.await.unwrap();
                assert_eq!(requests.len(), if enabled { 2 } else { 1 });
                if enabled {
                    assert_eq!(requests[0], requests[1]);
                }
            }
        }
    }
}

#[tokio::test]
async fn history_retries_only_incomplete_chunks() {
    for length in [Some(1), None] {
        let path = RepoPathBuf::from_string("a".into()).unwrap();
        let chunks: Vec<_> = [1, 2]
            .into_iter()
            .map(|node| HistoryResponseChunk {
                path: path.clone(),
                entries: (node..node + if length.is_some() { 1 } else { 2 })
                    .map(|node| WireHistoryEntry {
                        node: HgId::from_byte_array([node; 20]),
                        ..Default::default()
                    })
                    .collect(),
            })
            .collect();
        let bodies: Vec<_> = chunks
            .iter()
            .map(|chunk| serde_cbor::to_vec(&chunk.clone().to_wire()).unwrap())
            .collect();
        let mut partial = bodies[0].clone();
        partial.extend_from_slice(&bodies[1][..bodies[1].len() / 2]);
        let (client, requests) = server(
            vec![
                Reply::body(partial, false),
                Reply::body(bodies[1].clone(), false),
            ],
            1,
        )
        .await;
        let keys: Vec<_> = chunks
            .iter()
            .map(|chunk| Key::new(chunk.path.clone(), chunk.entries[0].node))
            .collect();
        let response = client.history(keys.clone(), length).await.unwrap();
        let entries: Vec<_> = response.entries.try_collect().await.unwrap();
        timeout(Duration::from_secs(5), response.stats)
            .await
            .unwrap()
            .unwrap();
        let expected: Vec<_> = chunks
            .into_iter()
            .flat_map(IntoIterator::into_iter)
            .collect();
        assert_eq!(entries, expected);
        let requests = requests.await.unwrap();
        let decode = |body: &[u8]| {
            serde_cbor::from_slice::<<HistoryRequest as ToWire>::Wire>(body)
                .unwrap()
                .to_api()
                .unwrap()
        };
        assert_eq!(
            decode(&requests[0])
                .keys
                .into_iter()
                .collect::<HashSet<_>>(),
            keys.clone().into_iter().collect()
        );
        assert_eq!(decode(&requests[1]).keys, vec![keys[1].clone()]);
        assert_eq!(decode(&requests[1]).length, length);
    }
}

#[tokio::test]
async fn zero_length_history_is_empty() {
    let client = HttpClientBuilder::new()
        .repo_name("test")
        .server_url("http://127.0.0.1:1".parse().unwrap())
        .build()
        .unwrap();
    assert!(
        client
            .history(vec![Key::default()], Some(0))
            .await
            .unwrap()
            .flatten()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn disabled_stream_retries_preserve_zero_length_history_request() {
    let (client, requests) = server_with_retries(vec![Reply::status(200)], 1, false).await;
    let keys = vec![Key::default(); 2];
    assert!(
        client
            .history(keys.clone(), Some(0))
            .await
            .unwrap()
            .flatten()
            .await
            .unwrap()
            .is_empty()
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = serde_cbor::from_slice::<<HistoryRequest as ToWire>::Wire>(&requests[0])
        .unwrap()
        .to_api()
        .unwrap();
    assert_eq!(request.keys, keys);
    assert_eq!(request.length, Some(0));
}

#[tokio::test]
async fn retry_limits_and_nonretryable_errors() {
    for (status, retries, count) in [(400, 1, 1), (503, 0, 1), (503, 1, 2)] {
        let (client, requests) =
            server((0..count).map(|_| Reply::status(status)).collect(), retries).await;
        let error = client
            .history(vec![Key::default()], None)
            .await
            .unwrap()
            .flatten()
            .await
            .unwrap_err();
        assert!(
            matches!(error, SaplingRemoteApiError::HttpError { status: actual, .. } if actual.as_u16() == status)
        );
        assert_eq!(requests.await.unwrap().len(), count);
    }
}

#[tokio::test]
async fn streaming_endpoints_retry_status_errors() {
    macro_rules! check {
        ($method:ident($($arg:expr),* $(,)?)) => {
            check!($method($($arg),*), false);
        };
        ($method:ident($($arg:expr),* $(,)?), $existing:expr) => {{
            for enabled in [true, false] {
                let should_retry = enabled || $existing;
                let mut replies = vec![Reply::status(503)];
                if should_retry {
                    replies.push(Reply::status(200));
                }
                let (client, requests) = server_with_retries(replies, 1, enabled).await;
                let result = client.$method($($arg),*).await.unwrap().flatten().await;
                assert_eq!(result.is_ok(), should_retry, "{}: {:?}", stringify!($method), result.err());
                let requests = requests.await.unwrap();
                assert_eq!(requests.len(), if should_retry { 2 } else { 1 }, "{}", stringify!($method));
                if should_retry {
                    assert_eq!(requests[0], requests[1], "{}", stringify!($method));
                }
            }
        }};
    }

    check!(path_history(
        HgId::default(),
        vec![RepoPathBuf::default()],
        None,
        vec![]
    ));
    check!(commit_revlog_data(vec![HgId::default()]));
    check!(upload_filenodes_batch(vec![HgFilenodeData::default()]));
    check!(upload_changesets(
        vec![UploadHgChangeset::default()],
        vec![]
    ));
    check!(upload_identical_changesets(vec![identical_changeset(
        HgId::default()
    )]));
    check!(commit_translate_id(
        vec![CommitId::default()],
        CommitIdScheme::Hg,
        None,
        None,
        None
    ));
    check!(blame(vec![Key::default()]));
    check!(check_path_permission(CheckPathPermissionRequest {
        paths: vec![RepoPathBuf::default()],
        ..Default::default()
    }));
    check!(check_manifest_permission(CheckManifestPermissionRequest {
        manifest_ids: vec![HgId::default()]
    }));
    check!(streaming_clone(None));
    check!(
        files_attrs(FetchContext::default(), vec![FileSpec::default()]),
        true
    );
    check!(
        trees(FetchContext::default(), vec![Key::default()], None),
        true
    );
}

fn identical_changeset(node_id: HgId) -> IdenticalChangesetContent {
    IdenticalChangesetContent {
        bcs_id: Default::default(),
        hg_parents: Default::default(),
        bonsai_parents: Default::default(),
        author: String::new(),
        time: 0,
        tz: 0,
        extras: vec![],
        bonsai_file_changes: vec![],
        hg_file_changes: vec![],
        message: String::new(),
        is_snapshot: false,
        hg_info: HgInfo {
            node_id,
            ..Default::default()
        },
        committer: None,
        committer_time: None,
        committer_tz: None,
        git_extra_headers: None,
        subtree_changes: None,
    }
}

#[tokio::test]
async fn keyed_endpoints_retry_only_pending_inputs() {
    macro_rules! check {
        ($method:ident($($arg:expr),* $(,)?), $expected:expr, $retry:expr) => {
            check!($method($($arg),*), $expected, $retry, |entry| {
                serde_cbor::to_vec(&entry.clone().to_wire()).unwrap()
            });
        };
        ($method:ident($($arg:expr),* $(,)?), $expected:expr, $retry:expr, $encode:expr) => {{
            let expected = $expected;
            let retry = $retry;
            let bodies: Vec<Vec<u8>> = expected.iter().map($encode).collect();
            let mut partial = bodies[0].clone();
            partial.extend_from_slice(&bodies[1][..bodies[1].len() / 2]);
            for enabled in [true, false] {
                let mut replies = vec![Reply::body(partial.clone(), false)];
                if enabled {
                    replies.push(Reply::body(bodies[1..].concat(), false));
                }
                let (client, requests) = server_with_retries(replies, 1, enabled).await;
                let response = client.$method($($arg),*).await.unwrap();
                let entries: Result<Vec<_>, _> = response.entries.try_collect().await;
                if enabled {
                    assert_eq!(entries.unwrap(), expected, "{} results", stringify!($method));
                } else {
                    assert!(entries.is_err(), "{}", stringify!($method));
                }
                let requests = requests.await.unwrap();
                assert_eq!(requests.len(), if enabled { 2 } else { 1 }, "{} request count", stringify!($method));
                if enabled {
                    let actual: serde_cbor::Value = serde_cbor::from_slice(&requests[1]).unwrap();
                    assert_eq!(actual, serde_cbor::value::to_value(&retry).unwrap(), "{} retry payload", stringify!($method));
                }
            }
        }};
    }

    let hgids: Vec<_> = [1, 2]
        .into_iter()
        .map(|n| HgId::from_byte_array([n; 20]))
        .collect();
    let paths: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|p| RepoPathBuf::from_string(p.into()).unwrap())
        .collect();
    let keys: Vec<_> = paths
        .iter()
        .zip(&hgids)
        .map(|(p, id)| Key::new(p.clone(), *id))
        .collect();
    let cursor: Vec<_> = paths
        .iter()
        .map(|path| PathHistoryRequestPaginationCursor {
            path: path.clone(),
            starting_commits: vec![(hgids[0], None)],
        })
        .collect();

    check!(
        path_history(hgids[0], paths.clone(), Some(5), cursor.clone()),
        paths
            .iter()
            .map(|path| PathHistoryResponse {
                path: path.clone(),
                entries: Ok(PathHistoryEntries::default())
            })
            .collect::<Vec<_>>(),
        PathHistoryRequest {
            commit: hgids[0],
            paths: vec![paths[1].clone()],
            limit: Some(5),
            cursor: vec![cursor[1].clone()]
        }
        .to_wire()
    );

    check!(
        blame(keys.clone()),
        keys.iter()
            .map(|key| BlameResult {
                file: key.clone(),
                data: Ok(BlameData::default())
            })
            .collect::<Vec<_>>(),
        BlameRequest {
            files: vec![keys[1].clone()]
        }
        .to_wire()
    );

    check!(
        commit_revlog_data(hgids.clone()),
        hgids
            .iter()
            .map(|id| CommitRevlogData {
                hgid: *id,
                revlog_data: Bytes::from_static(b"commit")
            })
            .collect::<Vec<_>>(),
        CommitRevlogDataRequest {
            hgids: vec![hgids[1]]
        },
        |entry: &CommitRevlogData| serde_cbor::to_vec(entry).unwrap()
    );

    let commits: Vec<_> = hgids.iter().map(|id| CommitId::Hg(*id)).collect();
    check!(
        commit_translate_id(
            commits.clone(),
            CommitIdScheme::Hg,
            Some("source".into()),
            Some("dest".into()),
            Some("lookup".into())
        ),
        commits
            .iter()
            .map(|commit| CommitTranslateIdResponse {
                commit: commit.clone(),
                translated: commit.clone()
            })
            .collect::<Vec<_>>(),
        CommitTranslateIdRequest {
            commits: vec![commits[1].clone()],
            scheme: CommitIdScheme::Hg,
            from_repo: Some("source".into()),
            to_repo: Some("dest".into()),
            lookup_behavior: Some("lookup".into())
        }
        .to_wire()
    );

    check!(
        check_path_permission(CheckPathPermissionRequest {
            hg_cs_id: hgids[0],
            paths: paths.clone()
        }),
        paths
            .iter()
            .map(|path| CheckPathPermissionResponse {
                path: path.clone(),
                result: Ok(CheckPathPermissionData::default())
            })
            .collect::<Vec<_>>(),
        CheckPathPermissionRequest {
            hg_cs_id: hgids[0],
            paths: vec![paths[1].clone()]
        }
        .to_wire()
    );

    check!(
        check_manifest_permission(CheckManifestPermissionRequest {
            manifest_ids: hgids.clone()
        }),
        hgids
            .iter()
            .map(|id| CheckManifestPermissionResponse {
                manifest_id: *id,
                ..Default::default()
            })
            .collect::<Vec<_>>(),
        CheckManifestPermissionRequest {
            manifest_ids: vec![hgids[1]]
        }
        .to_wire()
    );

    let filenodes: Vec<_> = hgids
        .iter()
        .map(|id| HgFilenodeData {
            node_id: *id,
            ..Default::default()
        })
        .collect();
    check!(
        upload_filenodes_batch(filenodes.clone()),
        hgids
            .iter()
            .map(|id| UploadTokensResponse {
                token: UploadToken::new_fake_token(AnyId::HgFilenodeId(*id), None)
            })
            .collect::<Vec<_>>(),
        Batch {
            batch: vec![UploadHgFilenodeRequest {
                data: filenodes[1].clone()
            }]
        }
        .to_wire()
    );

    let hgids: Vec<_> = [3, 1, 2]
        .into_iter()
        .map(|n| HgId::from_byte_array([n; 20]))
        .collect();
    let changesets: Vec<_> = hgids
        .iter()
        .enumerate()
        .map(|(i, id)| UploadHgChangeset {
            node_id: *id,
            changeset_content: HgChangesetContent {
                parents: if i == 0 {
                    Parents::None
                } else {
                    Parents::One(hgids[i - 1])
                },
                ..Default::default()
            },
        })
        .collect();
    let mutations = vec![HgMutationEntryContent {
        successor: hgids[0],
        predecessors: vec![HgId::from_byte_array([4; 20])],
        ..Default::default()
    }];
    let tokens = || {
        hgids
            .iter()
            .map(|id| UploadTokensResponse {
                token: UploadToken::new_fake_token(AnyId::HgChangesetId(*id), None),
            })
            .collect::<Vec<_>>()
    };
    check!(
        upload_changesets(changesets.clone(), mutations.clone()),
        tokens(),
        UploadHgChangesetsRequest {
            changesets: changesets[1..].to_vec(),
            mutations: mutations.clone()
        }
        .to_wire()
    );

    let identical: Vec<_> = hgids.iter().map(|id| identical_changeset(*id)).collect();
    check!(
        upload_identical_changesets(identical.clone()),
        tokens(),
        UploadIdenticalChangesetsRequest {
            changesets: identical[1..].to_vec()
        }
        .to_wire()
    );
}

fn clone_body(entries: &[StreamingChangelogData]) -> Vec<u8> {
    entries
        .iter()
        .flat_map(|data| {
            serde_cbor::to_vec(
                &StreamingChangelogResponse {
                    data: Ok(data.clone()),
                }
                .to_wire(),
            )
            .unwrap()
        })
        .collect()
}

#[tokio::test]
async fn streaming_clone_retries_partial_body_and_verifies_prefix() {
    let prefix = vec![
        StreamingChangelogData::Metadata(Metadata {
            index_size: 3,
            data_size: 3,
        }),
        StreamingChangelogData::IndexBlobChunk(StreamingChangelogBlob {
            chunk: Bytes::from_static(b"abc"),
            chunk_id: 0,
        }),
    ];
    for (change_prefix, short_prefix) in [(false, false), (true, false), (false, true)] {
        let mut complete = prefix.clone();
        if change_prefix {
            complete[1] = StreamingChangelogData::IndexBlobChunk(StreamingChangelogBlob {
                chunk: Bytes::from_static(b"xyz"),
                chunk_id: 0,
            });
        }
        complete.push(StreamingChangelogData::DataBlobChunk(
            StreamingChangelogBlob {
                chunk: Bytes::from_static(b"def"),
                chunk_id: 0,
            },
        ));
        if short_prefix {
            complete.truncate(1);
        }
        let (client, requests) = server(
            vec![
                Reply::incomplete_cbor(clone_body(&prefix)),
                Reply::body(clone_body(&complete), false),
            ],
            1,
        )
        .await;
        let result = client.streaming_clone(None).await.unwrap().flatten().await;
        if change_prefix || short_prefix {
            assert!(matches!(
                result,
                Err(SaplingRemoteApiError::IncompleteResponse(_))
            ));
        } else {
            let data: Vec<_> = result
                .unwrap()
                .into_iter()
                .map(|entry| entry.data.unwrap())
                .collect();
            assert_eq!(data, complete);
        }
        assert_eq!(requests.await.unwrap().len(), 2);
    }
}

#[tokio::test]
async fn streaming_clone_retries_multiple_interruptions() {
    let complete = vec![
        StreamingChangelogData::Metadata(Metadata {
            index_size: 6,
            data_size: 3,
        }),
        StreamingChangelogData::IndexBlobChunk(StreamingChangelogBlob {
            chunk: Bytes::from_static(b"abc"),
            chunk_id: 0,
        }),
        StreamingChangelogData::IndexBlobChunk(StreamingChangelogBlob {
            chunk: Bytes::from_static(b"def"),
            chunk_id: 1,
        }),
        StreamingChangelogData::DataBlobChunk(StreamingChangelogBlob {
            chunk: Bytes::from_static(b"ghi"),
            chunk_id: 0,
        }),
    ];
    let (client, requests) = server(
        vec![
            Reply::incomplete_cbor(clone_body(&complete[..2])),
            Reply::incomplete_cbor(clone_body(&complete[..1])),
            Reply::incomplete_cbor(clone_body(&complete[..3])),
            Reply::body(clone_body(&complete), false),
        ],
        3,
    )
    .await;
    let result = client
        .streaming_clone(Some("snapshot".into()))
        .await
        .unwrap()
        .flatten()
        .await
        .unwrap();
    assert_eq!(
        result
            .into_iter()
            .map(|entry| entry.data.unwrap())
            .collect::<Vec<_>>(),
        complete
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| *request == requests[0]));
}
