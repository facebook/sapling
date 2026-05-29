/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use curl::multi::Multi;
use flume::Receiver;
use flume::Sender;
use futures::FutureExt;
use futures::TryFutureExt;
use futures::channel::oneshot;

use crate::StatsFuture;
use crate::client::WorkerClient;
use crate::driver::MultiDriver;
use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::request::RequestId;
use crate::request::StreamRequest;
use crate::stats::Stats;

pub(crate) trait AsyncRequestDispatcher: Send + Sync {
    fn dispatch(
        &self,
        client: WorkerClient,
        requests: Vec<StreamRequest>,
    ) -> Result<StatsFuture, HttpClientError>;
}

pub(crate) struct SpawnBlockingDispatcher;

impl AsyncRequestDispatcher for SpawnBlockingDispatcher {
    fn dispatch(
        &self,
        client: WorkerClient,
        requests: Vec<StreamRequest>,
    ) -> Result<StatsFuture, HttpClientError> {
        let task = async_runtime::spawn_blocking(move || client.stream(requests));
        Ok(task.err_into::<HttpClientError>().map(|res| res?).boxed())
    }
}

pub(crate) fn spawn_blocking_dispatcher() -> Arc<dyn AsyncRequestDispatcher> {
    Arc::new(SpawnBlockingDispatcher)
}

pub(crate) fn multi_worker_dispatcher(worker_count: usize) -> Arc<dyn AsyncRequestDispatcher> {
    Arc::new(MultiWorkerDispatcher::new(worker_count))
}

// Fixed worker set for async requests. Each worker thread owns one curl multi.
// Dispatch prefers worker 0 when it is idle so serial traffic keeps using one
// warm multi, then round-robins bursts across all workers.
struct MultiWorkerDispatcher {
    workers: Vec<Worker>,
    next_worker: AtomicUsize,
    handles: Vec<JoinHandle<()>>,
}

// Publisher-side handle for a worker thread.
struct Worker {
    jobs: Option<Sender<HttpJob>>,
    // Queued or running batches assigned to this worker. A `WorkerReservation`
    // holds this count until the job fails to send or its batch finishes.
    load: Arc<AtomicUsize>,
}

impl Worker {
    fn reserve(&self) -> WorkerReservation {
        self.load.fetch_add(1, Ordering::AcqRel);
        WorkerReservation {
            load: self.load.clone(),
        }
    }

    fn try_reserve_idle(&self) -> Option<WorkerReservation> {
        self.load
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .ok()
            .map(|_| WorkerReservation {
                load: self.load.clone(),
            })
    }

    fn send(&self, job: HttpJob) -> Result<(), ()> {
        match self.jobs.as_ref() {
            Some(jobs) => jobs.send(job).map_err(|_| ()),
            None => Err(()),
        }
    }
}

// RAII guard for `Worker::load`.
struct WorkerReservation {
    load: Arc<AtomicUsize>,
}

impl Drop for WorkerReservation {
    fn drop(&mut self) {
        self.load.fetch_sub(1, Ordering::AcqRel);
    }
}

struct WorkerMulti {
    multi: Multi,
}

impl WorkerMulti {
    fn new() -> Self {
        Self {
            multi: Multi::new(),
        }
    }

    fn reset(&mut self) {
        self.multi = Multi::new();
    }

    fn configure(&mut self, config: &crate::client::Config) -> Result<(), HttpClientError> {
        self.multi
            .set_max_total_connections(config.max_concurrent_requests.unwrap_or(0))?;
        if let Some(max_streams) = config.max_concurrent_streams {
            self.multi.set_max_concurrent_streams(max_streams)?;
        }
        Ok(())
    }
}

impl std::ops::Deref for WorkerMulti {
    type Target = Multi;

    fn deref(&self) -> &Self::Target {
        &self.multi
    }
}

impl std::ops::DerefMut for WorkerMulti {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.multi
    }
}

struct HttpJob {
    client: WorkerClient,
    requests: Vec<StreamRequest>,
    stats_tx: oneshot::Sender<Result<Stats, HttpClientError>>,
    reservation: WorkerReservation,
}

struct ActiveBatch {
    client: WorkerClient,
    pending_requests: VecDeque<StreamRequest>,
    allowed_requests: usize,
    in_flight: usize,
    total_requests: usize,
    progress: Arc<BatchProgress>,
    stats_tx: Option<oneshot::Sender<Result<Stats, HttpClientError>>>,
    // Keeps the worker load nonzero until this batch finishes or fails.
    _reservation: WorkerReservation,
}

struct BatchProgress {
    start: Instant,
    first_activity: OnceLock<Instant>,
    downloaded: AtomicUsize,
    uploaded: AtomicUsize,
}

impl BatchProgress {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            first_activity: OnceLock::new(),
            downloaded: AtomicUsize::new(0),
            uploaded: AtomicUsize::new(0),
        }
    }

    fn note_first_activity(&self) {
        let _ = self.first_activity.set(Instant::now());
    }

    fn to_stats(&self, requests: usize) -> Stats {
        let latency = self
            .first_activity
            .get()
            .copied()
            .unwrap_or(self.start)
            .duration_since(self.start);
        Stats {
            downloaded: self.downloaded.load(Ordering::Relaxed),
            uploaded: self.uploaded.load(Ordering::Relaxed),
            requests,
            time: self.start.elapsed(),
            latency,
        }
    }
}

impl MultiWorkerDispatcher {
    fn new(worker_count: usize) -> Self {
        // Keep submission unbounded so `send_async()` remains guaranteed async at the API
        // boundary. This matches the old dispatcher semantics, which also allowed an
        // unbounded number of outstanding request batches via spawned tasks.
        assert!(worker_count > 0);
        let (workers, handles) = (0..worker_count)
            .map(|index| {
                let (jobs, rx) = flume::unbounded();
                let handle = std::thread::Builder::new()
                    .name(format!("sl-http-client-{}", index))
                    .spawn(move || run_dispatcher_worker(rx))
                    .expect("failed to start http dispatcher worker");
                (
                    Worker {
                        jobs: Some(jobs),
                        load: Arc::new(AtomicUsize::new(0)),
                    },
                    handle,
                )
            })
            .unzip();
        Self {
            workers,
            next_worker: AtomicUsize::new(0),
            handles,
        }
    }

    fn send_to_worker(
        &self,
        client: WorkerClient,
        requests: Vec<StreamRequest>,
        stats_tx: oneshot::Sender<Result<Stats, HttpClientError>>,
    ) -> Result<(), HttpClientError> {
        let (worker_index, reservation) =
            if let Some(reservation) = self.workers[0].try_reserve_idle() {
                (0, reservation)
            } else {
                let worker_index =
                    self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
                (worker_index, self.workers[worker_index].reserve())
            };

        if self.workers[worker_index]
            .send(HttpJob {
                client,
                requests,
                stats_tx,
                reservation,
            })
            .is_err()
        {
            return Err(anyhow!("http dispatcher worker terminated unexpectedly").into());
        }
        Ok(())
    }
}

impl AsyncRequestDispatcher for MultiWorkerDispatcher {
    fn dispatch(
        &self,
        client: WorkerClient,
        requests: Vec<StreamRequest>,
    ) -> Result<StatsFuture, HttpClientError> {
        let (stats_tx, stats_rx) = oneshot::channel();
        self.send_to_worker(client, requests, stats_tx)?;
        Ok(stats_rx.map(|res| res?).boxed())
    }
}

impl Drop for MultiWorkerDispatcher {
    fn drop(&mut self) {
        for worker in &mut self.workers {
            drop(worker.jobs.take());
        }
        for worker in self.handles.drain(..) {
            if let Err(err) = worker.join() {
                tracing::error!(
                    panic_message = panic_message(err.as_ref()),
                    "http dispatcher worker panicked during shutdown"
                );
            }
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else {
        payload
            .downcast_ref::<String>()
            .map(|message| message.as_str())
            .unwrap_or("<non-String payload>")
    }
}

fn run_dispatcher_worker(rx: Receiver<HttpJob>) {
    let mut multi = WorkerMulti::new();
    let mut next_batch_id = 0usize;

    while let Ok(first_job) = rx.recv() {
        let config = first_job.client.config.clone();
        if let Err(err) = multi.configure(&config) {
            fail_job(first_job, &err);
            fail_queued_jobs(&rx, &err);
            continue;
        }

        let driver = MultiDriver::new(&multi, config.verbose_stats);
        let batches = RefCell::new(HashMap::new());
        let requests_to_batches = RefCell::new(HashMap::new());
        let batch_id_counter = Cell::new(next_batch_id);
        let mut tls_error = false;

        enqueue_job(first_job, &mut batches.borrow_mut(), &mut next_batch_id);
        while let Ok(job) = rx.try_recv() {
            enqueue_job(job, &mut batches.borrow_mut(), &mut next_batch_id);
        }
        batch_id_counter.set(next_batch_id);

        let run_result = (|| -> Result<(), HttpClientError> {
            loop {
                try_add_pending_requests(
                    &driver,
                    &mut batches.borrow_mut(),
                    &mut requests_to_batches.borrow_mut(),
                )?;

                if batches.borrow().is_empty() {
                    break Ok(());
                }

                if driver.num_transfers() == 0 {
                    // All request slots are held by other workers. Wait briefly for a
                    // slot to free up rather than dropping the batch.
                    std::thread::sleep(Duration::from_millis(1));
                    while let Ok(job) = rx.try_recv() {
                        let mut next_batch_id = batch_id_counter.get();
                        enqueue_job(job, &mut batches.borrow_mut(), &mut next_batch_id);
                        batch_id_counter.set(next_batch_id);
                    }
                    continue;
                }

                driver.perform_with(
                    |res| {
                        if let Err((_, err)) = &res {
                            let err: HttpClientError = err.clone().into();
                            if let HttpClientError::Tls(_) = err {
                                tls_error = true;
                            }
                        }

                        {
                            let mut batches = batches.borrow_mut();
                            let mut requests_to_batches = requests_to_batches.borrow_mut();
                            handle_completed_request(
                                res,
                                &mut batches,
                                &mut requests_to_batches,
                                &driver,
                            )
                        }
                        .map_err(|err| Abort::WithReason(err.into()))
                    },
                    || {
                        while let Ok(job) = rx.try_recv() {
                            let mut next_batch_id = batch_id_counter.get();
                            enqueue_job(job, &mut batches.borrow_mut(), &mut next_batch_id);
                            batch_id_counter.set(next_batch_id);
                        }
                        next_batch_id = batch_id_counter.get();
                        try_add_pending_requests(
                            &driver,
                            &mut batches.borrow_mut(),
                            &mut requests_to_batches.borrow_mut(),
                        )
                    },
                )?;
            }
        })();

        drop(driver);

        if tls_error {
            multi.reset();
        }

        if let Err(err) = run_result {
            fail_all_batches(&mut batches.borrow_mut(), &err);
        } else {
            debug_assert!(batches.borrow().is_empty());
        }
    }
}

fn enqueue_job(job: HttpJob, batches: &mut HashMap<usize, ActiveBatch>, next_batch_id: &mut usize) {
    let batch_id = *next_batch_id;
    *next_batch_id += 1;

    let total_requests = job.requests.len();
    let allowed_requests = job
        .client
        .config
        .max_concurrent_requests_per_batch
        .unwrap_or(total_requests);

    batches.insert(
        batch_id,
        ActiveBatch {
            client: job.client,
            pending_requests: job.requests.into(),
            allowed_requests,
            in_flight: 0,
            total_requests,
            progress: Arc::new(BatchProgress::new()),
            stats_tx: Some(job.stats_tx),
            _reservation: job.reservation,
        },
    );
}

fn try_add_pending_requests(
    driver: &MultiDriver,
    batches: &mut HashMap<usize, ActiveBatch>,
    requests_to_batches: &mut HashMap<RequestId, usize>,
) -> Result<(), HttpClientError> {
    loop {
        let mut made_progress = false;

        for (batch_id, batch) in batches.iter_mut() {
            let want = batch.allowed_requests.min(batch.pending_requests.len());
            if want == 0 {
                continue;
            }

            let claims = batch.client.claimer.try_claim_requests(want);
            if claims.is_empty() {
                continue;
            }

            made_progress = true;
            batch.allowed_requests -= claims.len();

            for claim in claims {
                let mut request = batch
                    .pending_requests
                    .pop_front()
                    .expect("claim count must not exceed pending requests");
                add_progress_listeners(&mut request, batch.progress.clone());
                batch
                    .client
                    .event_listeners
                    .trigger_new_request(request.request.ctx_mut());

                let request_id = request.request.id();
                driver.add(request.into_easy(claim)?)?;
                requests_to_batches.insert(request_id, *batch_id);
                batch.in_flight += 1;
            }
        }

        if !made_progress {
            return Ok(());
        }
    }
}

fn handle_completed_request(
    res: Result<crate::Easy2H, (crate::Easy2H, curl::Error)>,
    batches: &mut HashMap<usize, ActiveBatch>,
    requests_to_batches: &mut HashMap<RequestId, usize>,
    driver: &MultiDriver,
) -> Result<(), HttpClientError> {
    let request_id = match &res {
        Ok(easy) => easy.get_ref().request_context().info().id(),
        Err((easy, _)) => easy.get_ref().request_context().info().id(),
    };

    let batch_id = requests_to_batches
        .remove(&request_id)
        .expect("completed transfer must belong to a batch");

    let batch_done = {
        let batch = batches
            .get_mut(&batch_id)
            .expect("completed transfer must reference a live batch");
        batch
            .client
            .report_result_and_drop_receiver(res)
            .map_err(HttpClientError::from)?;
        batch.allowed_requests += 1;
        batch.in_flight -= 1;
        batch.pending_requests.is_empty() && batch.in_flight == 0
    };

    if batch_done {
        finish_batch(batch_id, batches);
    }

    try_add_pending_requests(driver, batches, requests_to_batches)
}

fn finish_batch(batch_id: usize, batches: &mut HashMap<usize, ActiveBatch>) {
    let mut batch = batches
        .remove(&batch_id)
        .expect("batch must exist when finishing");
    let stats = batch.progress.to_stats(batch.total_requests);
    batch.client.event_listeners.trigger_stats(&stats);
    if let Some(stats_tx) = batch.stats_tx.take() {
        let _ = stats_tx.send(Ok(stats));
    }
}

fn add_progress_listeners(request: &mut StreamRequest, progress: Arc<BatchProgress>) {
    let listeners = request.request.ctx_mut().event_listeners();
    listeners.on_download_bytes({
        let progress = progress.clone();
        move |_req, n| {
            progress.downloaded.fetch_add(n, Ordering::Relaxed);
        }
    });
    listeners.on_upload_bytes({
        let progress = progress.clone();
        move |_req, n| {
            progress.uploaded.fetch_add(n, Ordering::Relaxed);
        }
    });
    listeners.on_first_activity(move |_req| {
        progress.note_first_activity();
    });
}

fn fail_all_batches(batches: &mut HashMap<usize, ActiveBatch>, err: &HttpClientError) {
    for (_, mut batch) in batches.drain() {
        fail_batch(&mut batch, err);
    }
}

fn fail_job(job: HttpJob, err: &HttpClientError) {
    let mut batch = ActiveBatch {
        client: job.client,
        pending_requests: job.requests.into(),
        allowed_requests: 0,
        in_flight: 0,
        total_requests: 0,
        progress: Arc::new(BatchProgress::new()),
        stats_tx: Some(job.stats_tx),
        _reservation: job.reservation,
    };
    fail_batch(&mut batch, err);
}

fn fail_queued_jobs(rx: &Receiver<HttpJob>, err: &HttpClientError) {
    while let Ok(job) = rx.try_recv() {
        fail_job(job, err);
    }
}

fn fail_batch(batch: &mut ActiveBatch, err: &HttpClientError) {
    let message = err.to_string();
    for mut request in batch.pending_requests.drain(..) {
        let _ = request.receiver.done(Err(anyhow!(message.clone()).into()));
    }
    if let Some(stats_tx) = batch.stats_tx.take() {
        let _ = stats_tx.send(Err(anyhow!(message).into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reservation_tracks_load() {
        let worker = Worker {
            jobs: None,
            load: Arc::new(AtomicUsize::new(0)),
        };

        let first = worker.try_reserve_idle().unwrap();
        assert_eq!(worker.load.load(Ordering::Acquire), 1);
        assert!(worker.try_reserve_idle().is_none());

        let second = worker.reserve();
        assert_eq!(worker.load.load(Ordering::Acquire), 2);

        drop(first);
        assert_eq!(worker.load.load(Ordering::Acquire), 1);

        drop(second);
        assert_eq!(worker.load.load(Ordering::Acquire), 0);
        assert!(worker.try_reserve_idle().is_some());
    }
}
