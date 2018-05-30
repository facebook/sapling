use action::CloudSyncTrigger;
use config::CommitCloudConfig;
use error::*;
use eventsource::reqwest::Client;
use num::FromPrimitive;
use reqwest::Url;
use serde_json;
use std::{str, thread, net::{SocketAddr, TcpListener}, path::PathBuf,
          sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, SystemTime}};
use util;

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

pub struct WorkspaceSubscriber {
    /// Server-Sent Events endpoint for Commit Cloud Live Notifications
    pub(crate) url: String,
    /// OAuth token valid for Commit Cloud Live Notifications
    pub(crate) access_token: String,
    /// Directory with connected subscribers
    pub(crate) connected_subscribers_path: PathBuf,
    /// Number of retries for `hg cloud sync`
    pub(crate) cloudsync_retries: u32,
    /// Tcp port to run a receiver
    pub(crate) tcp_receiver_port: u16,
    /// throttling rate for logging alive notification
    pub(crate) alive_throttling_rate_sec: u64,
    /// throttling rate for logging errors
    pub(crate) error_throttling_rate_sec: u64,
    /// throttling rate for logging no active subscriptions
    pub(crate) no_subs_throttling_rate_sec: u64,
}

// Enum stores last command id in an atomic usize
// (to allow threads to join)
enum_from_primitive! {
#[derive(Debug, PartialEq)]
enum CommandIds {
    None = 0,
    Restart = 1,
    Stop = 2,
}
}
// Commands
pub const RESTART: &'static str = "restart";
pub const STOP: &'static str = "stop";
#[derive(Default, Debug, Deserialize)]
pub struct Command(pub (String,));

struct ThrottlingExecutor {
    /// throttling rate in seconds
    rate: u64,
    /// last time of command execution
    last_time: SystemTime,
}

impl ThrottlingExecutor {
    pub fn new(rate_sec: u64) -> ThrottlingExecutor {
        ThrottlingExecutor {
            rate: rate_sec,
            last_time: SystemTime::now() - Duration::new(rate_sec, 0),
        }
    }
    /// Run command if it is time, skip otherwise
    #[inline]
    fn execute(&mut self, f: &Fn()) {
        let now = SystemTime::now();
        if now.duration_since(self.last_time)
            .map(|res| res.as_secs() >= self.rate)
            .unwrap_or(true)
        {
            f();
            self.last_time = now;
        }
    }
    /// Reset time to pretend the command last execution was a while ago
    #[inline]
    fn reset(&mut self) {
        self.last_time = SystemTime::now() - Duration::new(self.rate, 0);
    }
}

impl WorkspaceSubscriber {
    pub fn try_new(config: &CommitCloudConfig) -> Result<WorkspaceSubscriber> {
        Ok(WorkspaceSubscriber {
            url: config.streaminggraph_url.clone().ok_or_else(|| {
                ErrorKind::CommitCloudConfigError("undefined 'streaminggraph_url'")
            })?,
            access_token: util::read_access_token(config)?,
            connected_subscribers_path: config.connected_subscribers_path.clone().ok_or_else(
                || ErrorKind::CommitCloudConfigError("undefined 'connected_subscribers_path'"),
            )?,
            cloudsync_retries: config.cloudsync_retries,
            tcp_receiver_port: config.tcp_receiver_port,
            alive_throttling_rate_sec: config.alive_throttling_rate_sec,
            error_throttling_rate_sec: config.error_throttling_rate_sec,
            no_subs_throttling_rate_sec: config.no_subs_throttling_rate_sec,
        })
    }

    /// Simple cross platform commands receiver working on a Tcp Socket
    /// Expected commands are in json format
    /// Example: ["restart", {some optional json request data}]

    fn run_commands_receiver(port: u16, command_id: Arc<AtomicUsize>) -> Result<()> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).unwrap();
        info!("(receiver) Starting listening on port {}", port);
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    match serde_json::from_reader::<_, Command>(stream) {
                        Ok(command) => match (command.0).0.as_ref() {
                            RESTART => {
                                info!("(receiver) Received restart command");
                                info!("(receiver) Restart can take a while because it is graceful");
                                command_id.store(CommandIds::Restart as usize, Ordering::Relaxed);
                            }
                            STOP => {
                                info!("(receiver) Received stop command");
                                info!("(receiver) Stop can take a while because it is graceful");
                                command_id.store(CommandIds::Stop as usize, Ordering::Relaxed);
                                info!("(receiver) Shut down the receiver");
                                break;
                            }
                            _ => {}
                        },
                        Err(_) => {}
                    };
                }
                Err(e) => error!("(receiver) Connection failed {}", e),
            }
        }
        Ok(())
    }

    /// This function starts a receiver thread to receive simple commands from the outside
    /// It also manages set of running subscriptions
    ///
    /// The workflow is very simple: the receiver thread accepts few simple commands
    ///     restart
    ///     stop
    /// If a command comes, it gracefully cancels all previous subscriptions
    /// (and restart if requested)
    ///
    /// Main use case:
    ///
    /// If a cient add itself as a new subscriber (hg cloud join),
    /// it is also client's responsibility to send the restart command
    /// Same for unsubscribing (hg cloud leave)
    ///
    /// All synchronization is done through an atomic variable

    pub fn run(&mut self) -> Result<()> {
        let command_id = Arc::new(AtomicUsize::new(CommandIds::Restart as usize));
        let commands_receiver = {
            let command_id = command_id.clone();
            let port = self.tcp_receiver_port;
            thread::spawn(move || WorkspaceSubscriber::run_commands_receiver(port, command_id))
        };
        let mut throttler_no_subs = ThrottlingExecutor::new(self.no_subs_throttling_rate_sec);
        loop {
            match CommandIds::from_usize(command_id.load(Ordering::Relaxed))
                .unwrap_or(CommandIds::None)
            {
                CommandIds::Stop => {
                    info!("All subscriptions has been canceled!");
                    break;
                }
                CommandIds::Restart => {
                    info!("All previous subscriptions has been canceled!");
                    info!("Updating subscriptions");
                    command_id.store(CommandIds::None as usize, Ordering::Relaxed);
                    throttler_no_subs.reset();
                    self.run_subscriptions(command_id.clone())?;
                }
                _ => {
                    tinfo!(
                        throttler_no_subs,
                        "No active subscriptions running. Wait in standby"
                    );

                    thread::sleep(Duration::new(1, 0));
                }
            }
        }
        let _ = commands_receiver.join();
        Ok(())
    }

    #[inline]
    fn join_time(command_id: Arc<AtomicUsize>) -> bool {
        let command =
            CommandIds::from_usize(command_id.load(Ordering::Relaxed)).unwrap_or(CommandIds::None);
        command == CommandIds::Restart || command == CommandIds::Stop
    }

    /// This function reads the list of current connected subscribers
    /// It starts all the requested subscriptions by simply runing a separate thread for each one
    /// All threads keep checking the command flag and will join gracefully if it is restart or stop

    fn run_subscriptions(&mut self, command_id: Arc<AtomicUsize>) -> Result<()> {
        let mut children = vec![];
        let url = Url::parse(&self.url)?;

        for (subscription, repo_roots) in
            util::read_subscriptions(&self.connected_subscribers_path)?
        {
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
            let command_id = command_id.clone();
            let alive_throttling_rate_sec = self.alive_throttling_rate_sec;
            let error_throttling_rate_sec = self.error_throttling_rate_sec;

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
                    if WorkspaceSubscriber::join_time(command_id.clone()) {
                        return;
                    }
                }
                info!("{} Start listening to notifications", sid);

                let mut throttler_alive = ThrottlingExecutor::new(alive_throttling_rate_sec);
                let mut throttler_error = ThrottlingExecutor::new(error_throttling_rate_sec);

                // the library handles automatic reconnection
                for event in client {
                    if WorkspaceSubscriber::join_time(command_id.clone()) {
                        return;
                    }
                    let event = event.map_err(|e| CommitCloudHttpError(format!("{}", e)));
                    if let Err(e) = event {
                        terror!(throttler_error, "{} {}. Continue...", sid, e);
                        throttler_alive.reset();
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
                        continue;
                    }

                    throttler_alive.reset();
                    throttler_error.reset();

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
                        if WorkspaceSubscriber::join_time(command_id.clone()) {
                            return;
                        }
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
