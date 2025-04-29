/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::sync::Arc;

use anyhow::Context;
use anyhow::anyhow;
use edenfs_error::Result;
use tokio::task::JoinSet;

use crate::client::EdenFsClient;

pub(crate) type RequestParam = Box<Arc<EdenFsClient>>;
pub(crate) type RequestResult = Box<(dyn Future<Output = Result<()>> + Send)>;

/// Each thrift endpoint that we want to stress test will have a corresponding request structure.
/// To implement stress testing for a type, you simply have to implement the RequestFactory trait
/// for that request type. The RequestFactory trait tells the stress test code how to issue a
/// request for each Thrift endpoint. The trait is needed since passing in arbitrary lambdas that
/// capture local variables causes tricky lifetime issues.
pub trait RequestFactory {
    fn make_request(&self) -> impl FnOnce(RequestParam) -> RequestResult;

    fn request_name(&self) -> &'static str;
}

fn sanity_check_requests(num_requests: usize, num_tasks: usize) -> usize {
    if num_tasks > num_requests {
        eprintln!(
            "Cannot specify more tasks ({}) than requests ({}).",
            num_tasks, num_requests
        );
        eprintln!(
            "Falling back to issuing {} requests on {} tasks.",
            num_tasks, num_tasks
        );
        num_tasks
    } else {
        num_requests
    }
}

fn print_update(total: usize, finished: &mut usize) {
    let update_cadence = total / 10;
    let update = format!("{}/{} tasks finished running", finished, total);
    if update_cadence == 0 || *finished % update_cadence == 0 {
        println!("{}", update);
    } else {
        tracing::debug!(update);
    }
    *finished += 1;
}

pub async fn send_requests<Factory>(
    client: Arc<EdenFsClient>,
    factory: Arc<Factory>,
    num_requests: usize,
    num_tasks: usize,
) -> Result<()>
where
    Factory: RequestFactory + Send + Sync + 'static,
{
    let num_requests = sanity_check_requests(num_requests, num_tasks);

    println!(
        "Beginning stress test of {} {} requests across {} tokio tasks",
        num_requests,
        factory.request_name(),
        num_tasks
    );
    println!("NOTE: This may take a while; monitor progress with 'eden debug log --tail'");

    let requests_per_task = num_requests / num_tasks;
    let mut handles: JoinSet<Result<()>> = JoinSet::new();
    tracing::trace!(
        "spawning {} tasks that will each issue {} requests",
        num_tasks,
        requests_per_task
    );
    for i in 0..num_tasks {
        let num_requests = if i == (num_tasks - 1) {
            requests_per_task
        } else {
            requests_per_task + (num_requests % num_tasks)
        };
        let factory = factory.clone();
        let client = client.clone();
        handles.spawn(async move {
            for _ in 0..num_requests {
                let factory = factory.clone();
                let request = factory.make_request();
                Box::into_pin(request(Box::new(client.clone()))).await?;
            }
            Ok(())
        });
    }

    let mut num_finished = 1usize;
    while let Some(response) = handles.join_next().await {
        response.with_context(|| anyhow!("Request failed"))??;
        print_update(num_tasks, &mut num_finished);
    }
    Ok(())
}
