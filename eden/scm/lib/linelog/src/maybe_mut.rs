/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
