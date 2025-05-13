/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use strum::IntoStaticStr;

#[derive(IntoStaticStr, Clone, Debug, PartialEq, Eq, Hash)]
#[strum(serialize_all = "kebab_case")]
pub enum UseCaseId {
    ExampleUseCase,
    MeerakatCli,
    #[strum(serialize = "edenfsctl")]
    EdenFsCtl,
    RedirectFfi,
    WatchActiveCommit,
    #[strum(serialize = "testifyd")]
    TestifyDaemon,
}
