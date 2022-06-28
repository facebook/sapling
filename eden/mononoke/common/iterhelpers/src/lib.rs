/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// In case iterator has 0 or 2+ items, fails with errors
/// produced by corresponding error factories
pub fn get_only_item<T, E, N, NE, M, ME>(
    items: impl IntoIterator<Item = T>,
    no_items_error: N,
    many_items_error: M,
) -> Result<T, E>
where
    N: FnOnce() -> NE,
    NE: Into<E>,
    M: FnOnce(T, T) -> ME,
    ME: Into<E>,
{
    let mut iter = items.into_iter();
    let maybe_first = iter.next();
    let maybe_second = iter.next();
    match (maybe_first, maybe_second) {
        (None, None) => Err(no_items_error().into()),
        (Some(only_item), None) => Ok(only_item),
        (Some(item1), Some(item2)) => Err(many_items_error(item1, item2).into()),
        (None, Some(_)) => panic!("iterator returns Some after None"),
    }
}

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
    use anyhow::anyhow;
    use anyhow::Error;

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

    #[test]
    fn test_success() {
        let v: Vec<u8> = vec![1];
        let r: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert_eq!(r.unwrap(), 1);
    }

    #[test]
    fn test_no_items() {
        let v: Vec<u8> = vec![];
        let res: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert!(res.is_err());
        assert_eq!(format!("{}", res.unwrap_err()), "no items".to_string());
    }

    #[test]
    fn test_too_many_items() {
        let v: Vec<u8> = vec![1, 2, 3];
        let res: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert!(res.is_err());
        assert_eq!(format!("{}", res.unwrap_err()), "many items".to_string());
    }

    #[test]
    fn test_too_many_items_args() {
        let v: Vec<u8> = vec![1, 2, 3];
        let too_many_items = |i1, i2| {
            assert_eq!(i1, 1);
            assert_eq!(i2, 2);
            anyhow!("many items")
        };
        let res: Result<_, Error> = get_only_item(v, || anyhow!("no items"), too_many_items);
        assert!(res.is_err());
    }
}
