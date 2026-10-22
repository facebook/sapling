/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use edenapi_types::WireHistoryEntry;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;

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
