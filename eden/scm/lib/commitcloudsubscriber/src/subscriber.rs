/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::str;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::Result;
use futures::stream::StreamExt;
use log::error;
use log::info;
use parking_lot::Mutex;
use reqwest::Url;
use reqwest_eventsource::Event;
use reqwest_eventsource::EventSource;
use serde::Deserialize;

use crate::action::CloudSyncTrigger;
use crate::config::CommitCloudConfig;
use crate::error::*;
use crate::receiver::CommandName;
use crate::receiver::CommandName::CommitCloudCancelSubscriptions;
use crate::receiver::CommandName::CommitCloudRestartSubscriptions;
use crate::receiver::CommandName::CommitCloudStartSubscriptions;
use crate::util;
use crate::ActionsMap;

#[derive(Deserialize)]
pub struct Notification {
    pub(crate) version: u64,
}

#[derive(PartialEq, Eq, Hash)]
pub struct Subscription {
    pub(crate) repo_name: String,
    pub(crate) workspace: String,
}

/// WorkspaceSubscriberService manages a set of running subscriptions
/// and trigger `hg cloud sync` on notifications
/// The workflow is simple:
/// * fire `hg cloud sync` on start in every repo
/// * read and start current set of subscriptions and
///     fire `hg cloud sync` on notifications
/// * fire `hg cloud sync` when connection recovers
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

    /// OAuth token path (optional) for access to Commit Cloud SSE endpoint
    pub(crate) user_token_path: Option<PathBuf>,

    /// Directory with connected subscribers
    pub(crate) connected_subscribers_path: PathBuf,

    /// Number of retries for `hg cloud sync`
    pub(crate) cloudsync_retries: u32,

    /// Channel for communication between threads
    pub(crate) channel: (mpsc::Sender<CommandName>, mpsc::Receiver<CommandName>),

    /// Interrupt barrier for joining threads
    pub(crate) interrupt: Arc<AtomicBool>,
}

impl WorkspaceSubscriberService {
    pub fn new(config: &CommitCloudConfig) -> Result<WorkspaceSubscriberService> {
        Ok(WorkspaceSubscriberService {
            notification_url: config.notification_url.clone().ok_or(
                ErrorKind::CommitCloudConfigError("undefined 'notification_url'"),
            )?,
            user_token_path: config.user_token_path.clone(),
            connected_subscribers_path: config.connected_subscribers_path.clone().ok_or(
                ErrorKind::CommitCloudConfigError("undefined 'connected_subscribers_path'"),
            )?,
            cloudsync_retries: config.cloudsync_retries,
            channel: mpsc::channel(),
            interrupt: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn actions(&self) -> ActionsMap {
        let mut actions = ActionsMap::new();
        actions.insert(CommitCloudRestartSubscriptions, {
            let sender = Mutex::new(self.channel.0.clone());
            let interrupt = self.interrupt.clone();
            Box::new(
                move || match sender.lock().send(CommitCloudRestartSubscriptions) {
                    Err(err) => error!(
                        "Send CommitCloudRestartSubscriptions via mpsc::channel failed, reason: {}",
                        err
                    ),
                    Ok(_) => {
                        info!("Restart subscriptions can take a while because it is graceful");
                        interrupt.store(true, Ordering::Relaxed);
                    }
                },
            )
        });
        actions.insert(CommitCloudCancelSubscriptions, {
            let sender = Mutex::new(self.channel.0.clone());
            let interrupt = self.interrupt.clone();
            Box::new(
                move || match sender.lock().send(CommitCloudCancelSubscriptions) {
                    Err(err) => error!(
                        "Send CommitCloudCancelSubscriptions via mpsc::channel failed with {}",
                        err
                    ),
                    Ok(_) => {
                        info!("Cancel subscriptions can take a while because it is graceful");
                        interrupt.store(true, Ordering::Relaxed);
                    }
                },
            )
        });
        actions.insert(CommitCloudStartSubscriptions, {
            let sender = Mutex::new(self.channel.0.clone());
            let interrupt = self.interrupt.clone();
            Box::new(
                move || match sender.lock().send(CommitCloudStartSubscriptions) {
                    Err(err) => error!(
                        "Send CommitCloudStartSubscriptions via mpsc::channel failed with {}",
                        err
                    ),
                    Ok(_) => {
                        info!("Starting subscriptions.");
                        interrupt.store(true, Ordering::Relaxed);
                    }
                },
            )
        });
        actions
    }

    pub fn serve(self) -> Result<tokio::task::JoinHandle<Result<()>>> {
        self.channel.0.send(CommitCloudStartSubscriptions)?;
        Ok(tokio::spawn(async move {
            info!("Starting CommitCloud Workspace Subscriber Service");
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
                                let _ = child.await;
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
                                let _ = child.await;
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
    /// It starts all the requested subscriptions by creating a separate async task for each one
    /// All tasks keep checking the interrupt flag and join gracefully if it is restart or stop

    fn run_subscriptions(
        &self,
        access_token: util::Token,
    ) -> Result<Vec<tokio::task::JoinHandle<()>>> {
        util::read_subscriptions(&self.connected_subscribers_path)?
            .into_iter()
            .map(|(subscription, repo_roots)| {
                self.run_subscription(access_token.clone(), subscription, repo_roots)
            })
            .collect::<Result<Vec<tokio::task::JoinHandle<()>>>>()
    }

    /// Helper function to run a single subscription

    fn run_subscription(
        &self,
        access_token: util::Token,
        subscription: Subscription,
        repo_roots: Vec<PathBuf>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let mut notification_url = Url::parse(&self.notification_url)?;

        let sid = format!("({} @ {})", subscription.repo_name, subscription.workspace);
        info!("{} Subscribing to {}", sid, notification_url);

        notification_url
            .query_pairs_mut()
            .append_pair("workspace", &subscription.workspace)
            .append_pair("repo_name", &subscription.repo_name)
            .append_pair("access_token", &access_token.token)
            .append_pair("token_type", &access_token.token_type.to_string());

        let mut es = EventSource::get(notification_url);

        info!("{} Spawn a task to handle the subscription", sid);

        let cloudsync_retries = self.cloudsync_retries;
        let interrupt = self.interrupt.clone();

        Ok(tokio::spawn(async move {
            info!("{} Task started...", sid);

            let fire = |reason: &'static str, version: Option<u64>| {
                for repo_root in repo_roots.iter() {
                    info!(
                        "{} Fire CloudSyncTrigger in '{}' {}",
                        sid,
                        repo_root.display(),
                        reason,
                    );
                    // log outputs, results and continue even if unsuccessful
                    let _res = CloudSyncTrigger::fire(
                        &sid,
                        repo_root,
                        cloudsync_retries,
                        version,
                        subscription.workspace.clone(),
                        format!("scm_daemon: {}", reason),
                    );
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

            while !interrupt.load(Ordering::Relaxed) {
                match tokio::time::timeout(Duration::from_millis(500), es.next()).await {
                    Ok(Some(event)) => {
                        let event =
                            event.map_err(|e| ErrorKind::CommitCloudHttpError(format!("{}", e)));

                        match event {
                            Err(e) => {
                                error!("{} Restarting subscriptions due to error: {}...", sid, e);
                                interrupt.store(true, Ordering::Relaxed);
                            }
                            Ok(Event::Open) => {
                                info!("{} EventSource connection open...", sid)
                            }
                            Ok(Event::Message(e)) => {
                                let data = e.data;
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
                                    "{} Notification to sync version {} (full message: {})",
                                    sid, notification.version, &data
                                );
                                fire("on new version notification", Some(notification.version));
                            }
                        }
                    }
                    _ => continue,
                }
            }
        }))
    }
}
