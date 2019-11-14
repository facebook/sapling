/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use failure::Fail;

#[derive(Debug, Fail)]
#[fail(display = "invalid arguments\n(use '--help' to get help)")]
pub struct InvalidArguments;
