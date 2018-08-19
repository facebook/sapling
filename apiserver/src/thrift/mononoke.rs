// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use apiserver_thrift::server::MononokeApiservice;

#[derive(Clone)]
pub struct MononokeAPIServiceImpl {}
impl MononokeApiservice for MononokeAPIServiceImpl {}
