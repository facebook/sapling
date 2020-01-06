/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Column<N> {
    Empty,
    Blocked,
    Reserved(N),
    Ancestor(N),
    Parent(N),
}

impl<N> Column<N>
where
    N: Clone,
{
    pub(crate) fn matches(&self, n: &N) -> bool
    where
        N: Eq,
    {
        match self {
            Column::Empty | Column::Blocked => false,
            Column::Reserved(o) => n == o,
            Column::Ancestor(o) => n == o,
            Column::Parent(o) => n == o,
        }
    }

    fn variant(&self) -> usize {
        match self {
            Column::Empty => 0,
            Column::Blocked => 1,
            Column::Reserved(_) => 2,
            Column::Ancestor(_) => 3,
            Column::Parent(_) => 4,
        }
    }

    pub(crate) fn merge(&mut self, other: &Column<N>) {
        if other.variant() > self.variant() {
            *self = other.clone();
        }
    }

    fn reset(&mut self) {
        match self {
            Column::Blocked => *self = Column::Empty,
            _ => {}
        }
    }
}

pub(crate) trait ColumnsExt<N> {
    fn find(&self, node: &N) -> Option<usize>;
    fn find_empty(&self, index: usize) -> Option<usize>;
    fn first_empty(&self) -> Option<usize>;
    fn new_empty(&mut self) -> usize;
    fn reset(&mut self);
}

impl<N> ColumnsExt<N> for Vec<Column<N>>
where
    N: Clone + Eq,
{
    fn find(&self, node: &N) -> Option<usize> {
        for (index, column) in self.iter().enumerate() {
            if column.matches(node) {
                return Some(index);
            }
        }
        None
    }

    fn find_empty(&self, index: usize) -> Option<usize> {
        if self.get(index) == Some(&Column::Empty) {
            return Some(index);
        }
        self.first_empty()
    }

    fn first_empty(&self) -> Option<usize> {
        for (i, column) in self.iter().enumerate() {
            if *column == Column::Empty {
                return Some(i);
            }
        }
        None
    }

    fn new_empty(&mut self) -> usize {
        self.push(Column::Empty);
        self.len() - 1
    }

    fn reset(&mut self) {
        for column in self.iter_mut() {
            column.reset();
        }
        while let Some(Column::Empty) = self.last() {
            self.pop();
        }
    }
}
