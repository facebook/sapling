// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::action::CloudSyncTrigger;
use crate::config::CommitCloudConfig;
use crate::error::*;
use crate::receiver::CommandName::{
    self, CommitCloudCancelSubscriptions, CommitCloudRestartSubscriptions,
    CommitCloudStartSubscriptions,
};
use crate::util;
use eventsource::reqwest::Client;
use failure::{bail, Fallible};
use log::{error, info, warn};
use reqwest::Url;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime};
use std::{str, thread};

#[allow(unused_macros)]
macro_rules! tinfo {
    ($throttler:expr, $($args:tt)+) => ( $throttler.execute(&|| {
                        info!($($args)+);
                    }))
}

#[allow(unused_macros)]
macro_rules! terror {
    ($throttler:expr, $($args:tt)+) => ( $throttler.execute(&|| {
                        error!($($args)+);
                    }))
}

#[derive(Deserialize)]
pub struct Notification {
    pub(crate) version: u64,
    pub(crate) new_heads: Option<Vec<String>>,
    pub(crate) removed_heads: Option<Vec<String>>,
}

#[derive(PartialEq, Eq, Hash)]
pub struct Subscription {
    pub(crate) repo_name: String,
    pub(crate) workspace: String,
}

struct ThrottlingExecutor {
    /// throttling rate in seconds
    rate: Duration,

    /// last time of command execution
    last_time: SystemTime,
}

impl ThrottlingExecutor {
    /// create ThrottlingExecutor with some duration
    pub fn new(rate: Duration) -> ThrottlingExecutor {
        ThrottlingExecutor {
            rate,
            last_time: SystemTime::now() - rate,
        }
    }
    /// Run function if it is time, skip otherwise
    #[inline]
    fn execute(&mut self, f: &Fn()) {
        let now = SystemTime::now();
        if now
            .duration_since(self.last_time)
            .map(|elapsed| elapsed >= self.rate)
            .unwrap_or(true)
        {
            f();
            self.last_time = now;
        }
    }
    /// Reset time to pretend the command last execution was a while ago
    #[inline]
    fn reset(&mut self) {
        self.last_time = SystemTime::now() - self.rate;
    }
}

/// WorkspaceSubscriberService manages a set of running subscriptions
/// and trigger `hg cloud sync` on notifications
/// The workflow is simple:
/// * fire `hg cloud sync` on start in every repo
/// * read and start current set of subscriptions and
///     fire `hg cloud sync` on notifications
/// * fire `hg cloud sync` when connection recovers (could missed notifications)
/// * also provide actions (callbacks) to a few TcpReceiver commands
///     the commands are:
///         "start_subscriptions"
///         "restart_subscriptions"
///         "cancel_subscriptions"
///     if a command comes, gracefully cancel all previous subscriptions
///     and restart if requested
///     main use case:
///     if a cient (hg) add itself as a new subscriber (hg cloud join),
///     it is also client's responsibility to send "restart_subscriptions" command
///     same for unsubscribing (hg cloud leave)
/// The serve function starts the service

pub struct WorkspaceSubscriberService {
    /// Server-Sent Events endpoint for Commit Cloud Notifications
    pub(crate) notification_url: String,

    /// Http endpoint for Commit Cloud requests
    pub(crate) service_url: String,

    /// OAuth token path (optional) for access to Commit Cloud SSE endpoint
    pub(crate) user_token_path: Option<PathBuf>,

    /// Directory with connected subscribers
    pub(crate) connected_subscribers_path: PathBuf,

    /// Number of retries for `hg cloud sync`
    pub(crate) cloudsync_retries: u32,

    /// Throttling rate for logging alive notification
    pub(crate) alive_throttling_rate: Duration,

    /// Throttling rate for logging errors
    pub(crate) error_throttling_rate: Duration,

    /// Channel for communication between threads
    pub(crate) channel: (mpsc::Sender<CommandName>, mpsc::Receiver<CommandName>),

    /// Interrupt barrier for joining threads
    pub(crate) interrupt: Arc<AtomicBool>,
}

impl WorkspaceSubscriberService {
    pub fn new(config: &CommitCloudConfig) -> Fallible<WorkspaceSubscriberService> {
        Ok(WorkspaceSubscriberService {
            notification_url: config
                .notification_url
                .clone()
                .ok_or_else(|| ErrorKind::CommitCloudConfigError("undefined 'notification_url'"))?,
            service_url: config
                .service_url
                .clone()
                .ok_or_else(|| ErrorKind::CommitCloudConfigError("undefined 'service_url'"))?,
            user_token_path: config.user_token_path.clone(),
            connected_subscribers_path: config.connected_subscribers_path.clone().ok_or_else(
                || ErrorKind::CommitCloudConfigError("undefined 'connected_subscribers_path'"),
            )?,
            cloudsync_retries: config.cloudsync_retries,
            alive_throttling_rate: Duration::new(config.alive_throttling_rate_sec, 0),
            error_throttling_rate: Duration::new(config.error_throttling_rate_sec, 0),
            channel: mpsc::channel(),
            interrupt: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn actions(&self) -> HashMap<CommandName, Box<Fn() + Send>> {
        let mut actions: HashMap<CommandName, Box<Fn() + Send>> = HashMap::new();
        actions.insert(CommitCloudRestartSubscriptions, {
            let sender = self.channel.0.clone();
            let interrupt = self.interrupt.clone();
            Box::new(move || match sender.send(CommitCloudRestartSubscriptions) {
                Err(err) => error!(
                    "Send CommitCloudRestartSubscriptions via mpsc::channel failed, reason: {}",
                    err
                ),
                Ok(_) => {
                    info!("Restart subscriptions can take a while because it is graceful");
                    interrupt.store(true, Ordering::Relaxed);
                }
            })
        });
        actions.insert(CommitCloudCancelSubscriptions, {
            let sender = self.channel.0.clone();
            let interrupt = self.interrupt.clone();
            Box::new(move || match sender.send(CommitCloudCancelSubscriptions) {
                Err(err) => error!(
                    "Send CommitCloudCancelSubscriptions via mpsc::channel failed with {}",
                    err
                ),
                Ok(_) => {
                    info!("Cancel subscriptions can take a while because it is graceful");
                    interrupt.store(true, Ordering::Relaxed);
                }
            })
        });
        actions.insert(CommitCloudStartSubscriptions, {
            let sender = self.channel.0.clone();
            let interrupt = self.interrupt.clone();
            Box::new(move || match sender.send(CommitCloudStartSubscriptions) {
                Err(err) => error!(
                    "Send CommitCloudStartSubscriptions via mpsc::channel failed with {}",
                    err
                ),
                Ok(_) => interrupt.store(true, Ordering::Relaxed),
            })
        });
        actions
    }

    pub fn serve(self) -> Fallible<thread::JoinHandle<Fallible<()>>> {
        self.channel.0.send(CommitCloudStartSubscriptions)?;
        Ok(thread::spawn(move || {
            info!("Starting CommitCloud WorkspaceSubscriberService");
            loop {
                let command = self.channel.1.recv_timeout(Duration::from_secs(60));
                match command {
                    Ok(CommitCloudCancelSubscriptions) => {
                        info!(
                            "All previous subscriptions have been canceled! \
                             Waiting for another commands..."
                        );
                        self.interrupt.store(false, Ordering::Relaxed);
                    }
                    Ok(CommitCloudRestartSubscriptions) => {
                        info!(
                            "All previous subscriptions have been canceled! \
                             Restarting subscriptions..."
                        );
                        self.interrupt.store(false, Ordering::Relaxed);
                        // start subscription threads
                        let access_token = util::read_access_token(&self.user_token_path);
                        if let Ok(access_token) = access_token {
                            let subscriptions = self.run_subscriptions(access_token)?;
                            for child in subscriptions {
                                let _ = child.join();
                            }
                        } else {
                            info!("User is not authenticated with Commit Cloud yet");
                            continue;
                        }
                    }
                    Ok(CommitCloudStartSubscriptions) => {
                        info!("Starting subscriptions...");
                        self.interrupt.store(false, Ordering::Relaxed);
                        let access_token = util::read_access_token(&self.user_token_path);
                        // start subscription threads
                        if let Ok(access_token) = access_token {
                            let subscriptions = self.run_subscriptions(access_token)?;
                            for child in subscriptions {
                                let _ = child.join();
                            }
                        } else {
                            info!("User is not authenticated with Commit Cloud yet");
                            continue;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if !util::read_subscriptions(&self.connected_subscribers_path)?.is_empty() {
                            self.channel.0.send(CommitCloudStartSubscriptions)?;
                        }
                        continue;
                    }
                    Err(e) => {
                        error!("Receive from mpsc::channel failed with {}", e);
                        bail!("Receive and wait on mpsc::channel failed with {}", e);
                    }
                }
            }
        }))
    }

    /// This helper function reads the list of current connected subscribers
    /// It starts all the requested subscriptions by simply runing a separate thread for each one
    /// All threads keep checking the interrupt flag and join gracefully if it is restart or stop

    fn run_subscriptions(
        &self,
        access_token: util::Token,
    ) -> Fallible<Vec<thread::JoinHandle<()>>> {
        util::read_subscriptions(&self.connected_subscribers_path)?
            .into_iter()
            .map(|(subscription, repo_roots)| {
                self.run_subscription(access_token.clone(), subscription, repo_roots)
            })
            .collect::<Fallible<Vec<thread::JoinHandle<()>>>>()
    }

    /// Helper function to run a single subscription

    fn run_subscription(
        &self,
        access_token: util::Token,
        subscription: Subscription,
        repo_roots: Vec<PathBuf>,
    ) -> Fallible<thread::JoinHandle<()>> {
        let mut notification_url = Url::parse(&self.notification_url)?;
        let service_url = Url::parse(&self.service_url)?;

        let sid = format!("({} @ {})", subscription.repo_name, subscription.workspace);
        info!("{} Subscribing to {}", sid, notification_url);

        notification_url
            .query_pairs_mut()
            .append_pair("workspace", &subscription.workspace)
            .append_pair("repo_name", &subscription.repo_name)
            .append_pair("access_token", &access_token.token)
            .append_pair("token_type", &access_token.token_type.to_string());

        let client = Client::new(notification_url);

        info!("{} Spawn a thread to handle the subscription", sid);

        let cloudsync_retries = self.cloudsync_retries;
        let alive_throttling_rate = self.alive_throttling_rate;
        let error_throttling_rate = self.error_throttling_rate;
        let interrupt = self.interrupt.clone();

        Ok(thread::spawn(move || {
            info!("{} Thread started...", sid);

            let fire = |reason: &'static str, version: Option<u64>| {
                if service_url.to_socket_addrs().is_err() {
                    warn!(
                        "{} Skip CloudSyncTrigger: failed to lookup address information {}",
                        sid, service_url
                    );
                    return;
                }
                for repo_root in repo_roots.iter() {
                    info!(
                        "{} Fire CloudSyncTrigger in '{}' {}",
                        sid,
                        repo_root.display(),
                        reason,
                    );
                    // log outputs, results and continue even if unsuccessful
                    let _res = CloudSyncTrigger::fire(&sid, repo_root, cloudsync_retries, version);
                    if interrupt.load(Ordering::Relaxed) {
                        break;
                    }
                }
            };

            fire("before starting subscription", None);
            if interrupt.load(Ordering::Relaxed) {
                return;
            }

            info!("{} Start listening to notifications", sid);

            let mut throttler_alive = ThrottlingExecutor::new(alive_throttling_rate);
            let mut throttler_error = ThrottlingExecutor::new(error_throttling_rate);
            let mut last_error = false;

            // the library handles automatic reconnection
            for event in client {
                if interrupt.load(Ordering::Relaxed) {
                    return;
                }

                let event = event.map_err(|e| CommitCloudHttpError(format!("{}", e)));
                if let Err(e) = event {
                    terror!(throttler_error, "{} {}. Continue...", sid, e);
                    throttler_alive.reset();
                    last_error = true;
                    if format!("{}", e).contains("401 Unauthorized") {
                        // interrupt execution earlier
                        // all subscriptions have to be restarted from scratch
                        interrupt.store(true, Ordering::Relaxed);
                    }
                    continue;
                }

                let data = event.unwrap().data;
                if data.is_empty() {
                    tinfo!(
                        throttler_alive,
                        "{} Received empty event. Subscription is alive",
                        sid
                    );
                    throttler_error.reset();
                    if last_error {
                        fire("after recover from error", None);
                        if interrupt.load(Ordering::Relaxed) {
                            return;
                        }
                    }
                    last_error = false;
                    continue;
                }

                throttler_alive.reset();
                throttler_error.reset();
                last_error = false;

                info!("{} Received new notification event", sid);
                let notification = serde_json::from_str::<Notification>(&data);
                if let Err(e) = notification {
                    error!(
                        "{} Unable to decode json data in the event, reason: {}. Continue...",
                        sid, e
                    );
                    continue;
                }
                let notification = notification.unwrap();
                info!(
                    "{} CommitCloud informs that the latest workspace version is {}",
                    sid, notification.version
                );
                if let Some(ref new_heads) = notification.new_heads {
                    if !new_heads.is_empty() {
                        info!("{} New heads:\n{}", sid, new_heads.join("\n"));
                    }
                }
                if let Some(ref removed_heads) = notification.removed_heads {
                    if !removed_heads.is_empty() {
                        info!("{} Removed heads:\n{}", sid, removed_heads.join("\n"));
                    }
                }
                fire("on new version notification", Some(notification.version));
                if interrupt.load(Ordering::Relaxed) {
                    return;
                }
            }
        }))
    }
}
