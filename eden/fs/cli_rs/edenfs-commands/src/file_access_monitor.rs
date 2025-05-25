/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File as FsFile;
use std::io::BufRead;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_utils::mount_point_for_path;
use glob::Pattern;
use hg_util::path::expand_path;
use serde::Deserialize;
use serde::Serialize;

use crate::ExitCode;
use crate::Subcommand;
use crate::get_edenfs_instance;

// This path should be the same as the path defined in
// EdenServiceHandler.cpp::semifuture_startFileAccessMonitor
// Change this with caution since FAM is running privileged.
const TMP_FAM_OUTPUT_DIR_PATH: &str = "/tmp/edenfs/fam/";

#[cfg(target_os = "macos")]
#[derive(Parser, Debug)]
#[clap(
    name = "file-access-monitor",
    alias = "fam",
    about = "File Access Monitor(FAM) to audit processes.\nAvailable only on macOS."
)]
pub struct FileAccessMonitorCmd {
    #[clap(subcommand)]
    subcommand: FileAccessMonitorSubcommand,
}

#[derive(Parser, Debug)]
#[clap(about = "Start File Access Monitor. File access events are logged to the output file.")]
struct StartCmd {
    #[clap(
        help = "A list of paths that FAM should use as filters when monitoring file access events.\nIf no path is provided, the eden mount point will be used if FAM is run under an eden repo. Otherwise, FAM exits.",
        short = 'p',
        long = "paths",
        required = false
    )]
    paths: Vec<String>,

    #[clap(
        help = "The path of the output file where the file access events are logged.",
        short = 'o',
        long = "output"
    )]
    output: Option<String>,

    #[clap(
        help = "When set, the command returns immediately, leaving FAM running in the background.\nTo stop it, run 'eden fam stop'.\nThis is required since Ctrl-C is not killing FAM and timeout is not supported for now.",
        short = 'b',
        long = "background",
        required = true
    )]
    background: bool,

    #[clap(
        help = "How long FAM should run in seconds. This should not be set when '--background' is set.",
        short = 't',
        long = "timeout",
        default_value = "30",
        conflicts_with = "background"
    )]
    timeout: u64,

    #[clap(help = "When set, the output file is uploaded and a link is returned.")]
    upload: bool,
}

#[async_trait]
impl crate::Subcommand for StartCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        // Check the temporary folder exists, otherwise create it
        let tmp_dir_path = PathBuf::from(TMP_FAM_OUTPUT_DIR_PATH);
        if !tmp_dir_path.exists() {
            std::fs::create_dir_all(tmp_dir_path)?;
        }

        let mut monitor_paths: Vec<PathBuf> = Vec::new();

        for path in &self.paths {
            monitor_paths.push(expand_path(path));
        }

        if monitor_paths.is_empty() {
            // check cwd and if it's an eden-managed path then we use the eden mount
            // point as the default path to monitor
            let cwd = std::env::current_dir()?;
            match mount_point_for_path(&cwd) {
                Some(mount_point) => {
                    println!(
                        "No monitor path provided.\nActive eden mount detected, monitoring {}",
                        mount_point.display()
                    );
                    monitor_paths.push(mount_point);
                }
                _ => {
                    println!(
                        "No monitor path provided and the current working directory is not managed by eden.\nFile Access Monitor existing.",
                    );
                    return Ok(1);
                }
            }
        }

        println!("Starting File Access Monitor");

        let output_path = self.output.as_ref().map(expand_path);

        let start_result = client
            .start_file_access_monitor(&monitor_paths, output_path, self.upload)
            .await?;

        println!("File Access Monitor started [pid {}]", start_result.pid);
        println!(
            "Temp output file path: {}",
            start_result.tmp_output_path.display()
        );

        if self.background {
            println!(
                "File Access Monitor is running in the background.\nTo stop, run 'eden fam stop'."
            );
            return Ok(0);
        }

        // TODO[lxw]: handle timeout

        stop_fam().await
    }
}

async fn stop_fam() -> Result<ExitCode> {
    let instance = get_edenfs_instance();
    let client = instance.get_client();

    let stop_result = client.stop_file_access_monitor().await?;
    println!("File Access Monitor stopped");
    // TODO: handle the case when the output file is specified
    println!(
        "Output file saved to {}",
        stop_result.specified_output_path.display()
    );

    if stop_result.should_upload {
        // TODO[lxw]: handle uploading outputfile
        println!("Upload not implemented yet");
        return Ok(1);
    }
    Ok(0)
}

#[derive(Parser, Debug)]
#[clap(about = "Stop File Access Monitor to audit processes.")]
struct StopCmd {}

#[async_trait]
impl crate::Subcommand for StopCmd {
    async fn run(&self) -> Result<ExitCode> {
        stop_fam().await
    }
}

#[derive(Parser, Debug)]
#[clap(about = "Read the output file and parse it to a summary of file access events.")]
struct ReadCmd {
    #[clap(
        help = "Path to the FAM output file. This file is generated by FAM when monitoring file system activity.",
        short = 'f',
        long = "fam-output-file",
        required = true
    )]
    fam_file: String,

    #[clap(
        help = "Path prefix to filter the output events even more. This is useful when you know what subfolders you are interested in.",
        short = 'p',
        long = "path",
        required = false
    )]
    maybe_path: Option<String>,

    #[clap(
        help = "Print verbose information about parsed events.",
        long = "verbose",
        required = false
    )]
    verbose: bool,

    #[clap(
        help = "Specify the maximum number of PIDs to be displayed in the output. If set to 0, all PIDs will be displayed.",
        short = 'k',
        required = false,
        default_value = "10"
    )]
    count: usize,

    #[clap(
        help = "Sort the PIDs by the number of file access events they have generated.",
        required = false,
        default_value = "true"
    )]
    sort_process: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileItem {
    path: String,
    truncated: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct File {
    source: Option<FileItem>,
    target: Option<FileItem>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Process {
    pid: u64,
    ppid: u64,
    uid: u64,
    ancestors: Vec<u64>,
    args: Vec<String>,
    command: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Event {
    event_type: String,
    file: File,
    process: Process,
    event_timestamp: u64,
}

#[derive(Default)]
struct FilterContext {
    path_pattern: Option<String>,
    pid: Option<u64>,
}

fn parse_events<R: BufRead>(reader: R, filter_context: FilterContext) -> Result<Vec<Event>> {
    let mut objects: Vec<Event> = Vec::new();
    let mut new_object = String::new();
    for line in reader.lines().map_while(Result::ok) {
        new_object.push_str(&line);
        if line == "}" {
            objects.push(serde_json::from_str(&new_object)?);
            new_object.clear();
        }
    }

    match &filter_context.path_pattern {
        Some(pattern) => {
            let glob_pattern = Pattern::new(pattern).unwrap();
            let is_glob = pattern.contains('*');

            objects.retain(|event| {
                // This should be impossible, but just in case
                if event.file.target.is_none() && event.file.source.is_none() {
                    return false;
                }

                let target_file_path = event.file.target.as_ref().map_or("", |f| &f.path);
                let source_file_path = event.file.source.as_ref().map_or("", |f| &f.path);
                if is_glob {
                    glob_pattern.matches(target_file_path) || glob_pattern.matches(source_file_path)
                } else {
                    // Handle exact path matching
                    target_file_path == pattern || source_file_path == pattern
                }
            });
        }
        None => {}
    }

    Ok(objects)
}

#[derive(Clone)]
struct ProcessInfo {
    pid: u64,
    ppid: u64,
    command: String,
    counter: u64,
}

fn sort_process_info(events: &[Event], k: usize) -> Vec<ProcessInfo> {
    let mut process_info: HashMap<u64, ProcessInfo> = HashMap::new();
    for event in events {
        let process = &event.process;
        let count = process_info.entry(process.pid).or_insert(ProcessInfo {
            pid: process.pid,
            ppid: process.ppid,
            command: process.command.clone(),
            counter: 0,
        });
        count.counter += 1;
    }

    let mut sorted_info: Vec<ProcessInfo> = process_info.into_values().collect();
    sorted_info.sort_by_key(|p| Reverse(p.counter));

    if k == 0 {
        sorted_info
    } else {
        sorted_info[..k.min(sorted_info.len())].to_vec()
    }
}

fn print_sorted_process_info(sorted_process_info_slice: &[ProcessInfo]) {
    // Print the top results
    println!("{:<6} | {:<7} | {:<7} | Command", "PID", "PPID", "Counts");
    println!(
        "{:<6}-|-{:<7}-|-{:<7}-|-{}",
        "-".repeat(6),
        "-".repeat(7),
        "-".repeat(7),
        "-".repeat(10)
    );

    for p in sorted_process_info_slice {
        println!(
            "{:<6} | {:<7} | {:<7} | {}",
            p.pid, p.ppid, p.counter, p.command
        );
    }
}

fn print_process_info(events: &[Event], k: usize) {
    let sorted_process_info = sort_process_info(events, k);
    print_sorted_process_info(&sorted_process_info);
}

#[async_trait]
impl crate::Subcommand for ReadCmd {
    async fn run(&self) -> Result<ExitCode> {
        // construct the path
        let fam_file = PathBuf::from(&self.fam_file);
        let file = FsFile::open(fam_file)?;
        let path_pattern = self.maybe_path.clone();
        let reader = BufReader::new(file);

        let events = parse_events(
            reader,
            FilterContext {
                path_pattern,
                pid: None,
            },
        )?;

        if self.verbose {
            println!("Parsed {} objects", events.len());
            println!("{:#?}", events);
        }

        if self.sort_process {
            print_process_info(&events, self.count);
        }
        Ok(0)
    }
}

#[derive(Parser, Debug)]
enum FileAccessMonitorSubcommand {
    Start(StartCmd),
    Stop(StopCmd),
    Read(ReadCmd),
}

#[async_trait]
impl Subcommand for FileAccessMonitorCmd {
    async fn run(&self) -> Result<ExitCode> {
        use FileAccessMonitorSubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            Start(cmd) => cmd,
            Stop(cmd) => cmd,
            Read(cmd) => cmd,
        };
        sc.run().await
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_parse_complete_event() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/test_file_open",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)), FilterContext::default());
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1);
    }

    #[test]
    fn test_parse_complete_events() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/test_file_open",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}

{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/test_file_open",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)), FilterContext::default());
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 2);
    }

    #[test]
    fn test_parse_incomplete_events() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/test_file_open",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}

{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/test_file_open",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/pyth
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)), FilterContext::default());
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1);
    }

    #[test]
    fn test_sort_pids() {
        fn make_event(pid: u64, ppid: u64) -> Event {
            Event {
                event_type: "NOTIFY_OPEN".to_string(),
                file: File {
                    source: None,
                    target: Some(FileItem {
                        path: "what".to_string(),
                        truncated: false,
                    }),
                },
                process: Process {
                    ancestors: vec![],
                    args: vec![],
                    command: "what".to_string(),
                    pid,
                    ppid,
                    uid: 67890,
                },
                event_timestamp: 1740024705,
            }
        }

        let events = vec![
            make_event(66778, 22309),
            make_event(980066, 11759),
            make_event(1, 2),
            make_event(1, 2),
            make_event(980066, 11759),
            make_event(980066, 11759),
            make_event(66778, 22309),
            make_event(980066, 11759),
            make_event(1, 2),
            make_event(980066, 11759),
            make_event(1, 2),
        ];

        let sorted_pids = sort_process_info(&events, 0);
        assert_eq!(sorted_pids.len(), 3);
        assert_eq!(sorted_pids[0].pid, 980066);
        assert_eq!(sorted_pids[1].pid, 1);
        assert_eq!(sorted_pids[2].pid, 66778);
    }

    #[test]
    fn test_filtering_event_paths_with_pattern() {
        let events = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/tmp/test_dir/file1.txt",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 11111,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024701
}
{
  "event_type": "NOTIFY_WRITE",
  "file": {
    "target": {
      "path": "/tmp/test_dir/subdir/file2.txt",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/bin/vim",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024702
}
{
  "event_type": "NOTIFY_READ",
  "file": {
    "target": {
      "path": "/tmp/other_dir/file3.log",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/bin/cat",
    "pid": 33333,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024703
}
{
  "event_type": "NOTIFY_RENAME",
  "file": {
    "source": {
      "path": "/tmp/test_dir/old.txt",
      "truncated": false
    },
    "target": {
      "path": "/tmp/test_dir/new.txt",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/bin/mv",
    "pid": 44444,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024704
}
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "target": {
      "path": "/var/log/system.log",
      "truncated": false
    }
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/bin/tail",
    "pid": 55555,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}
"#;

        // Test cases:

        // 1. Exact path matching
        let exact_path = "/tmp/test_dir/file1.txt";
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext {
                path_pattern: Some(exact_path.to_string()),
                pid: None,
            },
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1);

        // 2. Glob pattern matching - all files in test_dir
        let glob_pattern = "/tmp/test_dir/*";
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext {
                path_pattern: Some(glob_pattern.to_string()),
                pid: None,
            },
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 3); // file1.txt, new.txt, old.txt->new.txt

        // 3. Glob pattern matching - all txt files
        let glob_pattern = "*.txt";
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext {
                path_pattern: Some(glob_pattern.to_string()),
                pid: None,
            },
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 3); // All .txt files

        // 4. Glob pattern matching - nested directories
        let glob_pattern = "/tmp/test_dir/*/file2.txt";
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext {
                path_pattern: Some(glob_pattern.to_string()),
                pid: None,
            },
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1); // Only file2.txt in subdir

        // 5. No matches
        let no_match_pattern = "/non/existent/path";
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext {
                path_pattern: Some(no_match_pattern.to_string()),
                pid: None,
            },
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 0);

        // 6. No pattern (should return all events)
        let parsed = parse_events(
            BufReader::new(Cursor::new(events)),
            FilterContext::default(),
        );
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 5);
    }
}
