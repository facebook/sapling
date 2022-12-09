/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl minitop

use std::collections::BTreeMap;
use std::io::stdout;
use std::io::Stdout;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use comfy_table::presets::UTF8_BORDERS_ONLY;
use comfy_table::Table;
use crossterm::cursor;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::queue;
use crossterm::style;
use crossterm::terminal;
use edenfs_client::EdenFsClient;
use edenfs_client::EdenFsInstance;
use edenfs_utils::humantime::HumanTime;
use edenfs_utils::humantime::TimeUnit;
use edenfs_utils::path_from_bytes;
use futures::FutureExt;
use futures::StreamExt;
use shlex::quote;
use sysinfo::Pid;
use sysinfo::System;
use sysinfo::SystemExt;
use thrift_types::edenfs::types::pid_t;
use thrift_types::edenfs::types::AccessCounts;
use thrift_types::edenfs::types::GetAccessCountsResult;

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

    #[clap(long, help = "Enable minitop interactive mode.")]
    interactive: bool,
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
    "PID",
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
    fn get_cmd_for_pid(&self, pid: pid_t) -> Result<String>;
}

impl GetAccessCountsResultExt for GetAccessCountsResult {
    fn get_cmd_for_pid(&self, pid: pid_t) -> Result<String> {
        match self.cmdsByPid.get(&pid) {
            Some(cmd) => {
                let cmd = String::from_utf8(cmd.to_vec())?;

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
    pid: pid_t,
    mount_name: String,
    cmd: String,
    access_counts: AccessCounts,
    fetch_counts: i64,
    last_access_time: Instant,
}

impl Process {
    fn new(pid: pid_t, mount_name: String) -> Self {
        Self {
            pid,
            mount_name,
            cmd: "<unknown>".to_string(),
            access_counts: AccessCounts::default(),
            fetch_counts: 0,
            last_access_time: Instant::now(),
        }
    }

    fn set_cmd(&mut self, cmd: String) -> &mut Self {
        self.cmd = cmd;
        self
    }

    /// Update this `Process` access counts.
    ///
    /// Since the `getAccessCounts` API gives us an incremental `AccessCounts`, this is simply
    /// incrementing the current counts with the passed ones. The last access time is also
    /// incremented.
    fn increment_access_counts(&mut self, counts: &AccessCounts) {
        self.access_counts.add(counts);
        self.last_access_time = Instant::now();
    }

    /// Update this `Process` fetch counts.
    ///
    /// As opposed to the access counts, this is an absolute value since EdenFS started, thus this
    /// will only update the last access time if the fetch counts also changed.
    fn set_fetch_counts(&mut self, fetch_counts: i64) {
        if self.fetch_counts != fetch_counts {
            self.fetch_counts = fetch_counts;
            self.last_access_time = Instant::now();
        }
    }

    /// Test if this `Process` is still running.
    fn is_running(&self, system: &System) -> bool {
        #[cfg(not(windows))]
        let pid = Pid::from(self.pid);
        #[cfg(windows)]
        let pid = Pid::from(self.pid as usize);
        system.process(pid).is_some()
    }
}

/// Get the last component of the passed in byte slice representing a Path.
///
/// The path is eagerly converted from an `OsString` to a `String` for ease of use.
fn get_mount_name(mount_path: &[u8]) -> anyhow::Result<String> {
    let path = path_from_bytes(mount_path)?;
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("filename is missing"))?;

    filename
        .to_os_string()
        .into_string()
        .map_err(|_| anyhow!("mount name is not UTF-8"))
}

type TrackedProcesses = BTreeMap<pid_t, Process>;

/// We aggregate all tracked processes in a separate step right before rendering
/// (as opposed to aggregating eagerly as we receive process logs in `update_process`)
/// because tracked processes could stop running which may change the top_pid.
fn aggregate_processes(processes: &TrackedProcesses, system: &System) -> Vec<Process> {
    // Technically, it's more correct to aggregate this by TGID
    // Because that's hard to get, we instead aggregate by mount & cmd
    // (mount, cmd) => Process
    let mut aggregated_processes = BTreeMap::<(&str, &str), Process>::new();

    for (_pid, process) in processes.iter() {
        match aggregated_processes.get_mut(&(&process.mount_name, &process.cmd)) {
            Some(agg_proc) => {
                // We aggregate access counts, but we don't change fetch counts
                // (this matches behavior in original python implementation)
                agg_proc.access_counts.add(&process.access_counts);

                // Figure out what the most relevant process id is
                if process.is_running(system)
                    || agg_proc.last_access_time < process.last_access_time
                {
                    agg_proc.pid = process.pid;
                    agg_proc.last_access_time = process.last_access_time;
                }
            }
            None => {
                aggregated_processes.insert((&process.mount_name, &process.cmd), process.clone());
            }
        }
    }

    let mut sorted_processes = aggregated_processes.into_values().collect::<Vec<Process>>();
    sorted_processes.sort_by(|a, b| b.last_access_time.cmp(&a.last_access_time));
    sorted_processes
}

struct ImportStat {
    count: i64,
    max_duration_us: i64,
}

async fn get_pending_import_counts(client: &EdenFsClient) -> Result<BTreeMap<String, ImportStat>> {
    let mut imports = BTreeMap::<String, ImportStat>::new();

    let counters = client.getRegexCounters(PENDING_COUNTER_REGEX).await?;
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
    let counters = client.getRegexCounters(LIVE_COUNTER_REGEX).await?;
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

struct TerminalAttributes {
    line_wrap_disabled: bool,
    alt_screen_entered: bool,
    raw_mode_entered: bool,
    stdout: std::io::Stdout,
}

impl TerminalAttributes {
    fn new() -> TerminalAttributes {
        let stdout = stdout();
        Self {
            line_wrap_disabled: false,
            alt_screen_entered: false,
            raw_mode_entered: false,
            stdout,
        }
    }

    fn disable_line_wrap(mut self) -> Result<TerminalAttributes> {
        queue!(self.stdout, terminal::DisableLineWrap)?;
        self.line_wrap_disabled = true;
        Ok(self)
    }

    fn enter_alt_screen(mut self) -> Result<TerminalAttributes> {
        queue!(self.stdout, terminal::EnterAlternateScreen)?;
        self.alt_screen_entered = true;
        Ok(self)
    }

    fn enter_raw_mode(mut self) -> Result<TerminalAttributes> {
        terminal::enable_raw_mode()?;
        self.raw_mode_entered = true;
        Ok(self)
    }
}

impl Drop for TerminalAttributes {
    fn drop(&mut self) {
        if self.line_wrap_disabled {
            let _ = queue!(self.stdout, terminal::EnableLineWrap);
        }

        if self.alt_screen_entered {
            let _ = queue!(self.stdout, terminal::LeaveAlternateScreen);
        }

        if self.raw_mode_entered {
            let _ = terminal::disable_raw_mode();
        }

        let _ = self.stdout.flush();
    }
}

struct Cursor {
    row: u16,
    terminal_rows: u16,
}

impl Cursor {
    fn new() -> Result<Self> {
        let (_, row) = cursor::position()?;
        let (_, terminal_rows) = terminal::size()?;

        Ok(Self { row, terminal_rows })
    }

    fn new_line(&mut self, stdout: &mut Stdout) -> Result<(), std::io::Error> {
        if self.row == self.terminal_rows {
            queue!(stdout, terminal::ScrollUp(1), cursor::MoveToColumn(1))
        } else {
            self.row += 1;
            queue!(stdout, cursor::MoveToNextLine(1))
        }
    }

    fn refresh_terminal_size(&mut self) -> Result<()> {
        let (_, terminal_rows) = terminal::size()?;
        self.terminal_rows = terminal_rows;

        // In the case where the terminal was resized and the cursor was on the last line, we want
        // to make sure we stay on the last line.
        if self.row > self.terminal_rows {
            self.row = self.terminal_rows;
        }

        Ok(())
    }
}

#[async_trait]
impl crate::Subcommand for MinitopCmd {
    async fn run(&self) -> Result<ExitCode> {
        let client = EdenFsInstance::global().connect(None).await?;
        let mut tracked_processes = TrackedProcesses::new();

        let mut system = System::new();

        // Setup rendering
        let mut attributes = TerminalAttributes::new()
            .disable_line_wrap()?
            .enter_raw_mode()?;
        if self.interactive {
            attributes = attributes.enter_alt_screen()?;
        }
        let _ = attributes; // silence warning

        let mut stdout = stdout();
        let mut cursor = Cursor::new()?;
        let mut events = EventStream::new();

        loop {
            if self.interactive {
                queue!(stdout, terminal::Clear(terminal::ClearType::All))?;
            }
            client.flushStatsNow();
            system.refresh_processes();
            cursor.refresh_terminal_size()?;

            // Update pending imports summary stats
            let (pending_imports, live_imports) = tokio::try_join!(
                get_pending_import_counts(&client),
                get_live_import_counts(&client)
            )?;

            // Update currently tracked processes (and add new ones if they haven't been tracked yet)
            let counts = client
                .getAccessCounts(self.refresh_rate.as_secs().try_into()?)
                .await?;

            for (mount, accesses) in &counts.accessesByMount {
                let mount_name = get_mount_name(mount)?;

                for (pid, access_counts) in &accesses.accessCountsByPid {
                    tracked_processes
                        .entry(*pid)
                        .or_insert_with(|| Process::new(*pid, mount_name.clone()))
                        .set_cmd(counts.get_cmd_for_pid(*pid)?)
                        .increment_access_counts(access_counts);
                }

                for (pid, fetch_counts) in &accesses.fetchCountsByPid {
                    tracked_processes
                        .entry(*pid)
                        .or_insert_with(|| Process::new(*pid, mount_name.clone()))
                        .set_cmd(counts.get_cmd_for_pid(*pid)?)
                        .set_fetch_counts(*fetch_counts);
                }
            }

            // Render pending trees/blobs
            for import_type in IMPORT_OBJECT_TYPES {
                let pending_counts = pending_imports
                    .get(&import_type.to_string())
                    .ok_or_else(|| anyhow!("Did not fetch pending {} info", import_type))?;
                let live_counts = live_imports
                    .get(&import_type.to_string())
                    .ok_or_else(|| anyhow!("Did not fetch live {} info", import_type))?;
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
                queue!(
                    stdout,
                    style::Print(format!("{:<40} {}", pending_string, live_string)),
                )?;
                cursor.new_line(&mut stdout)?;
            }

            // Render aggregated processes
            let mut table = Table::new();
            table.set_header(COLUMN_TITLES);
            table.load_preset(UTF8_BORDERS_ONLY);
            for aggregated_process in aggregate_processes(&tracked_processes, &system) {
                table.add_row(vec![
                    aggregated_process.pid.to_string(),
                    aggregated_process.mount_name.clone(),
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
                            .try_into()?,
                    ))
                    .simple_human_time(TimeUnit::Nanoseconds),
                    HumanTime::from(aggregated_process.last_access_time.elapsed())
                        .simple_human_time(TimeUnit::Seconds),
                    aggregated_process.cmd,
                ]);
            }

            for line in table.lines() {
                queue!(stdout, style::Print(line),)?;
                cursor.new_line(&mut stdout)?;
            }
            cursor.new_line(&mut stdout)?;
            cursor.new_line(&mut stdout)?;
            stdout.flush()?;

            loop {
                let delay = tokio::time::sleep(self.refresh_rate);
                let event = events.next().fuse();

                tokio::select! {
                    _ = delay => { break }
                    maybe_event = event => {
                        match maybe_event {
                            Some(event) => {
                                let event = event?;

                                let q = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
                                let ctrlc = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
                                if event == q || event == ctrlc {
                                    return Ok(0);
                                }
                            },
                            None => break,
                        }
                    }
                };
            }
        }
    }
}
