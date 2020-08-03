/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub type Chunker<T> = Box<dyn Fn(Vec<T>) -> Vec<Vec<T>>>;
