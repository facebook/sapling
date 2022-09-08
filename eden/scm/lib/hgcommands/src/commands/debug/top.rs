/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use clidispatch::io::IsTty;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use comfy_table::Table;
use debugtop::TableGenerator;

use super::Repo;
use super::Result;

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

pub fn run(ctx: ReqCtx<DebugTopOpts>, repo: &mut Repo) -> Result<u8> {
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

    loop {
        let mut table = Table::new();
        table.set_header(table_generator.column_titles());
        let entries = runlog::FileStore::entry_iter(repo.shared_dot_hg_path())?;
        for row in table_generator.generate_rows(entries, chrono::offset::Utc::now) {
            table.add_row(row);
        }
        if !running_in_tty {
            write!(stdout, "{}\n", table)?;
            break;
        }
        ctx.core
            .io
            .set_progress_str(format!("{}\n", table).as_str())?;
        sleep(Duration::from_millis(refresh_rate));
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugtop"
}

pub fn doc() -> &'static str {
    "outputs information about all running commands for the current repository"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
