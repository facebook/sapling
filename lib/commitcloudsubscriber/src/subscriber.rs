use action::CloudSyncTrigger;
use config::CommitCloudConfig;
use error::*;
use eventsource::reqwest::Client;
use reqwest::Url;
use serde_json;
use std::{collections::HashMap, path::PathBuf};
use std::thread;
use util;

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

pub struct WorkspaceSubscriber {
    /// Server-Sent Events endpoint for Commit Cloud Live Notifications
    pub(crate) url: String,
    /// OAuth token valid for Commit Cloud Live Notifications
    pub(crate) access_token: String,
    /// Map from a subscription to list of repo roots
    pub(crate) subscriptions: HashMap<Subscription, Vec<PathBuf>>,
    pub(crate) cloudsync_retries: u32,
}

impl WorkspaceSubscriber {
    // build subscriber for the set of repo/workspace pair
    pub fn try_new(config: &CommitCloudConfig) -> Result<WorkspaceSubscriber> {
        Ok(WorkspaceSubscriber {
            url: config
                .streaminggraph_url
                .clone()
                .ok_or_else(|| CommitCloudHttpError("undefined streaminggraph_url".into()))?,
            access_token: util::read_access_token(config)?,
            subscriptions: util::read_subscriptions(config)?,
            cloudsync_retries: config.cloudsync_retries,
        })
    }

    // start a separate thread for each subscription
    pub fn run(&mut self) -> Result<()> {
        let mut children = vec![];
        let url = Url::parse(&self.url)?;

        for (subscription, repo_roots) in &self.subscriptions {
            let mut url = url.clone();
            let sid = format!("({} @ {})", subscription.repo_name, subscription.workspace);
            info!("{} Subscribing to {}", sid, url);

            url.query_pairs_mut()
                .append_pair("workspace", &subscription.workspace)
                .append_pair("repo_name", &subscription.repo_name)
                .append_pair("access_token", &self.access_token);

            let client = Client::new(url);

            info!("{} Spawn a thread to handle the subscription", sid);

            let repo_roots = repo_roots.clone();
            let cloudsync_retries = self.cloudsync_retries;

            children.push(thread::spawn(move || {
                info!("{} Thread started...", sid);
                for repo_root in repo_roots.iter() {
                    info!(
                        "{} Fire CloudSyncTrigger in '{}' before starting subscription",
                        sid,
                        repo_root.display()
                    );
                    // log outputs, results and continue even if unsuccessful
                    let _res = CloudSyncTrigger::fire(&sid, repo_root, cloudsync_retries, None);
                }
                info!("{} Start listening to notifications", sid);
                // the library handles automatic reconnection
                for event in client {
                    let event = event.map_err(|e| CommitCloudHttpError(format!("{}", e)));
                    if let Err(e) = event {
                        error!("{} {}. Continue...", sid, e);
                        continue;
                    }
                    let data = event.unwrap().data;
                    if data.is_empty() {
                        info!("{} Received empty event. Continue...", sid);
                        continue;
                    }
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
                            info!("New heads:\n{}", new_heads.join("\n"));
                        }
                    }
                    if let Some(ref removed_heads) = notification.removed_heads {
                        if !removed_heads.is_empty() {
                            info!("Removed heads:\n{}", removed_heads.join("\n"));
                        }
                    }
                    for repo_root in repo_roots.iter() {
                        info!("{} Fire CloudSyncTrigger in '{}'", sid, repo_root.display());
                        // log outputs, results and continue even if unsuccessful
                        let _res = CloudSyncTrigger::fire(
                            &sid,
                            repo_root,
                            cloudsync_retries,
                            Some(notification.version),
                        );
                    }
                }
            }));
        }
        for child in children {
            let _ = child.join();
        }
        Ok(())
    }
}
