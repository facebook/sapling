/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use ascii_tree::row::Alignment;
use ascii_tree::row::Row;
use ascii_tree::row::Rows;
use clidispatch::ReqCtx;
use clidispatch::io::IsTty;
use cliparser::define_flags;
use cmdutil::Result;
use debugtop::TableGenerator;
use repo::repo::Repo;

define_flags! {
    pub struct DebugTopOpts {
        /// rate to refresh in milliseconds
        #[short('r')]
        refresh_rate: i64 = 1000,

        /// amount of milliseconds to show a process after it finishes
        #[short('d')]
        reap_delay: i64 = 2000,

        /// columns separated by comma; shows all if none is specified
        #[short('c')]
        columns: String = "",
    }
}

pub fn run(ctx: ReqCtx<DebugTopOpts>, repo: &Repo) -> Result<u8> {
    let mut stdout = ctx.io().output();
    let mut stderr = ctx.io().error();
    let running_in_tty = stdout.is_tty();
    let refresh_rate = ctx.opts.refresh_rate.max(0) as u64;
    let reap_delay = chrono::Duration::milliseconds(ctx.opts.reap_delay);

    let mut table_generator = match TableGenerator::new(ctx.opts.columns, reap_delay) {
        Err(unexpected_columns) => {
            for column in unexpected_columns.iter() {
                write!(stderr, "Error: column \"{}\" was not expected\n", column)?;
            }
            return Ok(22);
        }
        Ok(table_generator) => table_generator,
    };

    let num_columns = table_generator.column_titles().len();

    loop {
        let entries = runlog::FileStore::entry_iter(repo.shared_dot_hg_path())?;
        let header = Row {
            columns: table_generator.column_titles().clone(),
        };
        let data_rows = table_generator.generate_rows(entries, chrono::offset::Utc::now);
        let rows = Rows {
            rows: std::iter::once(header)
                .chain(data_rows.map(|columns| Row { columns }))
                .collect(),
            column_alignments: vec![Alignment::Left; num_columns],
            column_min_widths: vec![0; num_columns],
            column_max_widths: vec![usize::MAX; num_columns],
        };

        if !running_in_tty {
            write!(stdout, "{}\n", rows)?;
            break;
        }
        ctx.core
            .io
            .set_progress_str(format!("{}\n", rows).as_str())?;
        sleep(Duration::from_millis(refresh_rate));
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugtop"
}

pub fn doc() -> &'static str {
    "outputs information about all running commands for the current repository"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
