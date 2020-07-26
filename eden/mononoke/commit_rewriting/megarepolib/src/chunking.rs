/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub type Chunker<T> = Box<dyn Fn(Vec<T>) -> Vec<Vec<T>>>;

/// Produce a chunker fn, which breaks a vector into `num_chunks` pieces,
/// such that piece sizes initially gradually grow, but after that are as
/// even as possible
pub fn gradually_increasing_chunker<T: Clone>(num_chunks: usize) -> Chunker<T> {
    Box::new(move |items: Vec<T>| {
        let sizes = get_gradual_chunk_sizes(num_chunks, items.len());
        let mut remainder = items.as_slice();
        let mut chunks: Vec<Vec<T>> = Vec::new();
        for current_size in sizes {
            let (iteration_slice, new_remainder) = remainder.split_at(current_size);
            let new_vec = iteration_slice.to_vec();
            chunks.push(new_vec);
            remainder = new_remainder
        }

        chunks
    })
}

/// Chunk `items` elements into `chunks` pieces as evenly as possible
/// Return the number of items in each chunk
fn fill_evenly(chunks: usize, mut items: usize) -> Vec<usize> {
    let mut sizes = vec![];
    let fill_in = (items / chunks) + if items % chunks == 0 { 0 } else { 1 };

    while items > 0 {
        if items > fill_in {
            sizes.push(fill_in);
            items -= fill_in;
        } else {
            sizes.push(items);
            items = 0;
        }
    }

    sizes
}

/// Chunk `num_items` elements into at most `num_chunks` pieces, where
/// the chunking first increases gradually, then is as even as possible
/// Return the number of elements in each chunk
fn get_gradual_chunk_sizes(num_chunks: usize, num_items: usize) -> Vec<usize> {
    let prefix_sizes = [1, 10, 100, 1000];
    let mut remaining_items = num_items;
    let mut sizes = vec![];

    let gradually_growing_prefixes = std::cmp::min(prefix_sizes.len(), num_chunks - 1);
    for i in 0..gradually_growing_prefixes {
        if remaining_items > prefix_sizes[i] {
            sizes.push(prefix_sizes[i]);
            remaining_items -= prefix_sizes[i];
        } else {
            sizes.push(remaining_items);
            remaining_items = 0;
            break;
        }
    }

    let remaining_chunks = num_chunks - sizes.len();

    if remaining_items == 0 {
        return sizes;
    }

    // `remaining_chunks` cannot be 0 here, as
    // `gradually_growing_prefixes` was bound by `num_chunks - 1`
    // Still, let's assert for easier debugging if something breaks it
    assert_ne!(
        remaining_chunks, 0,
        "Logic error: filled all the chunks ({}) with less than all items ({})",
        num_chunks, num_items,
    );

    sizes.extend(fill_evenly(remaining_chunks, remaining_items));
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_gradual_chunk_sizes() {
        assert_eq!(get_gradual_chunk_sizes(1, 1), vec![1]);
        assert_eq!(get_gradual_chunk_sizes(2, 1), vec![1]);
        assert_eq!(get_gradual_chunk_sizes(2, 2), vec![1, 1]);
        assert_eq!(get_gradual_chunk_sizes(3, 10), vec![1, 9]);
        assert_eq!(get_gradual_chunk_sizes(4, 10), vec![1, 9]);
        assert_eq!(get_gradual_chunk_sizes(1, 10), vec![10]);
        assert_eq!(get_gradual_chunk_sizes(2, 10), vec![1, 9]);
        assert_eq!(get_gradual_chunk_sizes(2, 11), vec![1, 10]);
        assert_eq!(get_gradual_chunk_sizes(2, 12), vec![1, 11]);
        assert_eq!(get_gradual_chunk_sizes(3, 12), vec![1, 10, 1]);
        assert_eq!(get_gradual_chunk_sizes(3, 110), vec![1, 10, 99]);
        assert_eq!(get_gradual_chunk_sizes(3, 111), vec![1, 10, 100]);
        assert_eq!(get_gradual_chunk_sizes(3, 112), vec![1, 10, 101]);
        assert_eq!(get_gradual_chunk_sizes(4, 1110), vec![1, 10, 100, 999]);
        assert_eq!(get_gradual_chunk_sizes(4, 1111), vec![1, 10, 100, 1000]);
        assert_eq!(get_gradual_chunk_sizes(4, 1112), vec![1, 10, 100, 1001]);
        assert_eq!(get_gradual_chunk_sizes(5, 1112), vec![1, 10, 100, 1000, 1]);
        assert_eq!(
            get_gradual_chunk_sizes(5, 10000),
            vec![1, 10, 100, 1000, 8889]
        );
        assert_eq!(
            get_gradual_chunk_sizes(6, 10000),
            vec![1, 10, 100, 1000, 4445, 4444]
        );

        let chunked = get_gradual_chunk_sizes(100, 1_000_000);
        assert_eq!(chunked.len(), 100);
        assert_eq!(chunked.iter().sum::<usize>(), 1_000_000);
        assert_eq!(chunked[0], 1);
        assert_eq!(chunked[1], 10);
        assert_eq!(chunked[2], 100);
        assert_eq!(chunked[3], 1000);
        assert_eq!(chunked[4], (1_000_000 - 1111) / 96 + 1);
        assert_eq!(chunked[98], (1_000_000 - 1111) / 96 + 1);
    }

    #[test]
    fn test_gradually_increasing_chunker() {
        let chunker = gradually_increasing_chunker(3);
        assert_eq!(chunker(vec![1]), vec![vec![1]]);
        assert_eq!(chunker(vec![1, 2]), vec![vec![1], vec![2]]);
        assert_eq!(chunker(vec![1, 2, 3, 4]), vec![vec![1], vec![2, 3, 4]]);

        let chunker = gradually_increasing_chunker(100);
        let v: Vec<u8> = vec![0; 1_000_000];
        let chunks = chunker(v);
        assert_eq!(chunks.len(), 100);
        assert_eq!(
            chunks.iter().map(|chunk| chunk.len()).sum::<usize>(),
            1_000_000
        );
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[2].len(), 100);
        assert_eq!(chunks[3].len(), 1000);
        assert_eq!(chunks[4].len(), (1_000_000 - 1111) / 96 + 1);
        assert_eq!(chunks[98].len(), (1_000_000 - 1111) / 96 + 1);
    }
}
