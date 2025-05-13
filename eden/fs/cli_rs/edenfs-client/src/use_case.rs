/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use strum::IntoStaticStr;

#[derive(Clone, Debug, Eq, IntoStaticStr, Hash, PartialEq)]
#[strum(serialize_all = "kebab_case")]
#[repr(u32)]
pub enum UseCaseId {
    #[strum(serialize = "edenfsctl")]
    EdenFsCtl,
    EdenFsTests,
    ExampleUseCase,
    MeerakatCli,
    RedirectFfi,
    #[strum(serialize = "testifyd")]
    TestifyDaemon,
    WatchActiveCommit,
}
