/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Break iterable down into chunks, by saturating an accumulator
pub fn chunk_by_accumulation<T, A: Copy>(
    items: impl IntoIterator<Item = T>,
    zero: A,
    add: impl Fn(A, &T) -> A,
    overflows: impl Fn(A) -> bool,
) -> Vec<Vec<T>> {
    let mut res = vec![];
    let mut acc = zero;
    let mut current = vec![];
    for item in items.into_iter() {
        let new_acc = add(acc, &item);
        if current.is_empty() || !overflows(new_acc) {
            current.push(item);
            acc = new_acc;
        } else {
            res.push(current);
            acc = add(zero, &item);
            current = vec![item];
        }
    }

    // current can only be empty
    // if the whole `items` was empty
    if !current.is_empty() {
        res.push(current);
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_by_accumulation_simple() {
        let v = vec![1, 2, 3, 1, 1, 1];
        let chunks = chunk_by_accumulation(v, 0, |a, x| a + x, |a| a > 3);
        assert_eq!(chunks, vec![vec![1, 2], vec![3], vec![1, 1, 1]]);
    }

    #[test]
    fn test_chunk_by_accumulation_one_item_overflows() {
        // even though 3 on its own overflows the accumulator,
        // we don't drop it
        let v = vec![1, 2, 3, 1, 1, 1];
        let chunks = chunk_by_accumulation(v, 0, |a, x| a + x, |a| a >= 3);
        assert_eq!(chunks, vec![vec![1], vec![2], vec![3], vec![1, 1], vec![1]]);
    }

    #[test]
    fn test_chunk_by_accumulation_empty() {
        let v = vec![];
        let chunks = chunk_by_accumulation(v, 0, |a, x| a + x, |a| a > 3);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_by_accumulation_single_item() {
        let v = vec![5];
        let chunks = chunk_by_accumulation(v, 0, |a, x| a + x, |a| a > 3);
        assert_eq!(chunks, vec![vec![5]]);
    }

    #[test]
    fn test_chunk_by_accumulation_all_fit_in_single_chunk() {
        let v = vec![1, 2, 3, 1, 1, 1];
        let chunks = chunk_by_accumulation(v.clone(), 0, |a, x| a + x, |a| a > 1000);
        assert_eq!(chunks, vec![v]);
    }
}
