/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::Deref;

pub(crate) enum MaybeMut<'a, T> {
    Ref(&'a T),
    Mut(&'a mut T),
}

impl<T> Deref for MaybeMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Ref(v) => v,
            Self::Mut(v) => v,
        }
    }
}

impl<'a, T> MaybeMut<'a, T> {
    pub(crate) fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Ref(_) => None,
            Self::Mut(v) => Some(v),
        }
    }

    pub(crate) fn is_mut(&self) -> bool {
        matches!(self, Self::Mut(_))
    }
}
