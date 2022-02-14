/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl minitop

use async_trait::async_trait;
use clap::Parser;
use comfy_table::{presets::UTF8_BORDERS_ONLY, Table};
use crossterm::execute;
use crossterm::terminal;
use shlex::quote;
use std::collections::BTreeMap;
use std::io::{stdout, Write};
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use edenfs_client::{EdenFsClient, EdenFsInstance};
use edenfs_error::{EdenFsError, Result, ResultExt};
use edenfs_utils::humantime::{HumanTime, TimeUnit};
use edenfs_utils::path_from_bytes;

use thrift_types::edenfs::types::{pid_t, AccessCounts, GetAccessCountsResult};

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Simple monitoring of EdenFS accesses by process.")]
pub struct MinitopCmd {
    // TODO: For minitop, we may want to allow querying for < 1s, but this
    // requires modifying the thrift call and the eden service itself.
    // < 1s may be more useful for the realtime stats we see in minitop/top.
    #[clap(
        long,
        short,
        help = "Specify the rate (in seconds) at which eden top updates.",
        default_value = "1",
        parse(from_str = parse_refresh_rate),
    )]
    refresh_rate: Duration,
}

fn parse_refresh_rate(arg: &str) -> Duration {
    let seconds = arg
        .parse::<u64>()
        .expect("Please enter a valid whole positive number for refresh_rate.");

    Duration::new(seconds, 0)
}

const PENDING_COUNTER_REGEX: &str = r"store\.hg\.pending_import\..*";
const LIVE_COUNTER_REGEX: &str = r"store\.hg\.live_import\..*";
const IMPORT_OBJECT_TYPES: &[&str] = &["blob", "tree"];
const STATS_NOT_AVAILABLE: i64 = 0;

const UNKNOWN_COMMAND: &str = "<unknown>";
const COLUMN_TITLES: &[&str] = &[
    "TOP PID",
    "MOUNT",
    "READS",
    "WRITES",
    "TOTAL COUNT",
    "FETCHES",
    "MEMORY",
    "DISK",
    "IMPORTS",
    "TIME SPENT",
    "LAST ACCESS",
    "CMD",
];

trait GetAccessCountsResultExt {
    fn get_cmd_for_pid(&self, pid: &i32) -> Result<String>;
}

impl GetAccessCountsResultExt for GetAccessCountsResult {
    fn get_cmd_for_pid(&self, pid: &i32) -> Result<String> {
        match self.cmdsByPid.get(pid) {
            Some(cmd) => {
                let cmd = String::from_utf8(cmd.to_vec()).from_err()?;

                // remove trailing null which would cause the command to show up with an
                // extra empty string on the end
                let cmd = cmd.trim_end_matches(char::from(0));

                let mut parts: Vec<&str> = cmd.split(char::from(0)).collect();
                let path = Path::new(parts[0]);
                if path.is_absolute() {
                    parts[0] = path
                        .file_name()
                        .ok_or_else(|| anyhow!("cmd filename is missing"))?
                        .to_str()
                        .ok_or_else(|| anyhow!("cmd is not UTF-8"))?;
                }

                Ok(parts
                    .into_iter()
                    .enumerate()
                    .map(|(i, part)| {
                        if i == 0 {
                            // the first item is the cmd
                            String::from(part)
                        } else {
                            quote(part).into_owned()
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(" "))
            }
            None => Ok(String::from(UNKNOWN_COMMAND)),
        }
    }
}

trait AccessCountsExt {
    fn add(&mut self, other: &AccessCounts);
}

impl AccessCountsExt for AccessCounts {
    fn add(&mut self, other: &AccessCounts) {
        self.fsChannelTotal += other.fsChannelTotal;
        self.fsChannelReads += other.fsChannelReads;
        self.fsChannelWrites += other.fsChannelWrites;
        self.fsChannelBackingStoreImports += other.fsChannelBackingStoreImports;
        self.fsChannelDurationNs += other.fsChannelDurationNs;
        self.fsChannelMemoryCacheImports += other.fsChannelMemoryCacheImports;
        self.fsChannelDiskCacheImports += other.fsChannelDiskCacheImports;
    }
}

#[derive(Clone)]
struct Process {
    pid: u32,
    mount: String,
    cmd: String,
    access_counts: AccessCounts,
    fetch_counts: u64,
    last_access_time: SystemTime,
}

impl Process {
    fn is_running(&self) -> bool {
        Path::new(&format!("/proc/{}", self.pid)).is_dir()
    }
}

struct TrackedProcesses {
    processes: BTreeMap<u32, Process>,
}

impl TrackedProcesses {
    fn new() -> Self {
        TrackedProcesses {
            processes: BTreeMap::<u32, Process>::new(),
        }
    }

    fn extract_mount(path: &[u8]) -> anyhow::Result<String> {
        let path = path_from_bytes(path)?;
        let filename = path
            .file_name()
            .ok_or_else(|| anyhow!("filename is missing"))?;

        filename
            .to_os_string()
            .into_string()
            .map_err(|_| anyhow!("mount name is not UTF-8"))
    }

    /// Starts to track a given process. If the process is already being tracked,
    /// then it updates the process's information (counts, last update time, etc).
    ///
    /// At any given time, a single pid may have multiple access logs.
    fn update_process(
        &mut self,
        pid: &pid_t,
        mount: &[u8],
        cmd: String,
        access_counts: AccessCounts,
        fetch_counts: &i64,
    ) -> Result<()> {
        let pid = u32::try_from(*pid).from_err()?;
        let mount = TrackedProcesses::extract_mount(mount)?;
        let fetch_counts = u64::try_from(*fetch_counts).from_err()?;

        match self.processes.get_mut(&pid) {
            Some(existing_proc) => {
                existing_proc.cmd = cmd;

                // We increment access counts, but overwrite fetch counts
                // (this matches behavior in original python implementation)
                existing_proc.access_counts.add(&access_counts);
                existing_proc.fetch_counts = fetch_counts;

                existing_proc.last_access_time = SystemTime::now();
            }
            None => {
                self.processes.insert(
                    pid,
                    Process {
                        pid,
                        mount,
                        cmd,
                        access_counts,
                        fetch_counts,
                        last_access_time: SystemTime::now(),
                    },
                );
            }
        }

        Ok(())
    }

    /// We aggregate all tracked processes in a separate step right before rendering
    /// (as opposed to aggregating eagerly as we receive process logs in `update_process`)
    /// because tracked processes could stop running which may change the top_pid.
    fn aggregated_processes(&self) -> Vec<Process> {
        // Technically, it's more correct to aggregate this by TGID
        // Because that's hard to get, we instead aggregate by mount & cmd
        // (mount, cmd) => Process
        let mut aggregated_processes = BTreeMap::<(&str, &str), Process>::new();

        for (_pid, process) in self.processes.iter() {
            match aggregated_processes.get_mut(&(&process.mount, &process.cmd)) {
                Some(agg_proc) => {
                    // We aggregate access counts, but we don't change fetch counts
                    // (this matches behavior in original python implementation)
                    agg_proc.access_counts.add(&process.access_counts);

                    // Figure out what the most relevant process id is
                    if process.is_running() || agg_proc.last_access_time < process.last_access_time
                    {
                        agg_proc.pid = process.pid;
                        agg_proc.last_access_time = process.last_access_time;
                    }
                }
                None => {
                    aggregated_processes.insert((&process.mount, &process.cmd), process.clone());
                }
            }
        }

        let mut sorted_processes = aggregated_processes.into_values().collect::<Vec<Process>>();
        sorted_processes.sort_by(|a, b| b.last_access_time.cmp(&a.last_access_time));
        sorted_processes
    }
}

struct ImportStat {
    count: i64,
    max_duration_us: i64,
}

async fn get_pending_import_counts(client: &EdenFsClient) -> Result<BTreeMap<String, ImportStat>> {
    let mut imports = BTreeMap::<String, ImportStat>::new();

    let counters = client
        .getRegexCounters(PENDING_COUNTER_REGEX)
        .await
        .from_err()?;
    for import_type in IMPORT_OBJECT_TYPES {
        let counter_prefix = format!("store.hg.pending_import.{}", import_type);
        let number_requests = counters
            .get(&format!("{}.count", counter_prefix))
            .unwrap_or(&STATS_NOT_AVAILABLE);
        let longest_outstanding_request_us = counters
            .get(&format!("{}.max_duration_us", counter_prefix))
            .unwrap_or(&STATS_NOT_AVAILABLE);

        imports.insert(
            import_type.to_string(),
            ImportStat {
                count: *number_requests,
                max_duration_us: *longest_outstanding_request_us,
            },
        );
    }

    Ok(imports)
}

async fn get_live_import_counts(client: &EdenFsClient) -> Result<BTreeMap<String, ImportStat>> {
    let mut imports = BTreeMap::<String, ImportStat>::new();
    let counters = client
        .getRegexCounters(LIVE_COUNTER_REGEX)
        .await
        .from_err()?;
    for import_type in IMPORT_OBJECT_TYPES {
        let single_prefix = format!("store.hg.live_import.{}", import_type);
        let batched_prefix = format!("store.hg.live_import.batched_{}", import_type);

        let count = counters
            .get(&format!("{}.count", single_prefix))
            .unwrap_or(&STATS_NOT_AVAILABLE)
            + counters
                .get(&format!("{}.count", batched_prefix))
                .unwrap_or(&STATS_NOT_AVAILABLE);
        let max_duration_us = std::cmp::max(
            counters
                .get(&format!("{}.max_duration_us", single_prefix))
                .unwrap_or(&STATS_NOT_AVAILABLE),
            counters
                .get(&format!("{}.max_duration_us", batched_prefix))
                .unwrap_or(&STATS_NOT_AVAILABLE),
        );

        imports.insert(
            import_type.to_string(),
            ImportStat {
                count,
                max_duration_us: *max_duration_us,
            },
        );
    }

    Ok(imports)
}

#[async_trait]
impl crate::Subcommand for MinitopCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        let mut tracked_processes = TrackedProcesses::new();

        // Setup rendering
        let mut stdout = stdout();
        execute!(stdout, terminal::DisableLineWrap).from_err()?;

        loop {
            client.flushStatsNow();

            // Update pending imports summary stats
            let (pending_imports, live_imports) = tokio::try_join!(
                get_pending_import_counts(&client),
                get_live_import_counts(&client)
            )?;

            // Update currently tracked processes (and add new ones if they haven't been tracked yet)
            let counts = client
                .getAccessCounts(self.refresh_rate.as_secs().try_into().from_err()?)
                .await
                .from_err()?;

            for (mount, accesses) in &counts.accessesByMount {
                for (pid, access_counts) in &accesses.accessCountsByPid {
                    tracked_processes.update_process(
                        pid,
                        mount,
                        counts.get_cmd_for_pid(pid)?,
                        access_counts.clone(),
                        accesses.fetchCountsByPid.get(pid).unwrap_or(&0),
                    )?;
                }

                for (pid, fetch_counts) in &accesses.fetchCountsByPid {
                    tracked_processes.update_process(
                        pid,
                        mount,
                        counts.get_cmd_for_pid(pid)?,
                        AccessCounts::default(),
                        fetch_counts,
                    )?;
                }
            }

            // Render pending trees/blobs
            for import_type in IMPORT_OBJECT_TYPES {
                let pending_counts =
                    pending_imports
                        .get(&import_type.to_string())
                        .ok_or_else(|| {
                            EdenFsError::Other(anyhow!(
                                "Did not fetch pending {} info",
                                import_type
                            ))
                        })?;
                let live_counts = live_imports.get(&import_type.to_string()).ok_or_else(|| {
                    EdenFsError::Other(anyhow!("Did not fetch live {} info", import_type))
                })?;
                let pending_string = format!(
                    "total pending {}: {} ({:.3}s)",
                    import_type,
                    pending_counts.count,
                    pending_counts.max_duration_us as f64 / 1000000.0
                );
                let live_string = format!(
                    "total live {}: {} ({:.3}s)",
                    import_type,
                    live_counts.count,
                    live_counts.max_duration_us as f64 / 1000000.0
                );
                stdout
                    .write(format!("{:<40} {}\n", pending_string, live_string).as_bytes())
                    .from_err()?;
            }

            // Render aggregated processes
            let mut table = Table::new();
            table.set_header(COLUMN_TITLES);
            table.load_preset(UTF8_BORDERS_ONLY);
            for aggregated_process in tracked_processes.aggregated_processes() {
                table.add_row(vec![
                    aggregated_process.pid.to_string(),
                    aggregated_process.mount.clone(),
                    aggregated_process.access_counts.fsChannelReads.to_string(),
                    aggregated_process.access_counts.fsChannelWrites.to_string(),
                    aggregated_process.access_counts.fsChannelTotal.to_string(),
                    aggregated_process.fetch_counts.to_string(),
                    aggregated_process
                        .access_counts
                        .fsChannelMemoryCacheImports
                        .to_string(),
                    aggregated_process
                        .access_counts
                        .fsChannelDiskCacheImports
                        .to_string(),
                    aggregated_process
                        .access_counts
                        .fsChannelBackingStoreImports
                        .to_string(),
                    HumanTime::from(Duration::from_nanos(
                        aggregated_process
                            .access_counts
                            .fsChannelDurationNs
                            .try_into()
                            .from_err()?,
                    ))
                    .simple_human_time(TimeUnit::Nanoseconds),
                    HumanTime::from(aggregated_process.last_access_time.elapsed().from_err()?)
                        .simple_human_time(TimeUnit::Seconds),
                    aggregated_process.cmd,
                ]);
            }

            stdout.write(table.to_string().as_bytes()).from_err()?;
            stdout.write("\n\n".as_bytes()).from_err()?;

            tokio::time::sleep(self.refresh_rate).await;
        }
    }
}
