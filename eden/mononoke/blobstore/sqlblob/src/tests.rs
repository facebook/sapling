/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use anyhow::Context;
use anyhow::Error;
use blobstore::DEFAULT_PUT_BEHAVIOUR;
use borrowed::borrowed;
use bytes::Bytes;
use fbinit::FacebookInit;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use rand::RngCore;
use strum::IntoEnumIterator;

async fn test_chunking_methods<Test, Fut>(
    fb: FacebookInit,
    put_behaviour: PutBehaviour,
    do_test: Test,
) -> Result<(), Error>
where
    Test: Fn(CoreContext, CountedSqlblob, Arc<TestSource>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    for allow_inline in [true, false] {
        let (test_source, config_store) = get_test_config_store();
        let blobstore =
            Sqlblob::with_sqlite_in_memory(put_behaviour, &config_store, allow_inline, 0)?;
        let ctx = CoreContext::test_mock(fb);
        do_test(ctx, blobstore, test_source)
            .await
            .with_context(|| format_err!("while testing allow_inline {}", allow_inline))?;
    }
    Ok(())
}

async fn read_write_size(
    fb: FacebookInit,
    put_behaviour: PutBehaviour,
    blob_size: usize,
) -> Result<(), Error> {
    test_chunking_methods(fb, put_behaviour, |ctx, bs, _| async move {
        borrowed!(ctx);
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key = format!("manifoldblob_test_{}", suffix);

        let mut bytes_in = vec![0u8; blob_size];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

        assert!(
            !bs.is_present(ctx, &key).await?.assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        // Write a fresh blob
        bs.put(ctx, key.clone(), blobstore_bytes).await?;
        // Read back and verify
        let bytes_out = bs.get(ctx, &key).await?;
        assert_eq!(&bytes_in.to_vec(), bytes_out.unwrap().as_raw_bytes());

        assert!(
            bs.is_present(ctx, &key).await?.assume_not_found_if_unsure(),
            "Blob should exist now"
        );
        Ok(())
    })
    .await
}

#[fbinit::test]
async fn read_write(fb: FacebookInit) -> Result<(), Error> {
    for put_behaviour in PutBehaviour::iter() {
        // test a range of sizes that are inlineable and not inlineable
        for size in [0, 1, 2, 3, 64, MAX_INLINE_LEN, 254, 255, 256, 512] {
            let blob_size: usize = size.try_into()?;
            read_write_size(fb, put_behaviour, blob_size)
                .await
                .with_context(|| format_err!("while testing size {}", size))?;
        }
    }
    Ok(())
}

#[fbinit::test]
async fn double_put(fb: FacebookInit) -> Result<(), Error> {
    test_chunking_methods(fb, DEFAULT_PUT_BEHAVIOUR, |ctx, bs, _| async move {
        borrowed!(ctx);
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key = format!("manifoldblob_test_{}", suffix);

        let mut bytes_in = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

        assert!(
            !bs.is_present(ctx, &key).await?.assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        // Write a fresh blob
        bs.put(ctx, key.clone(), blobstore_bytes.clone()).await?;
        // Write it again
        bs.put(ctx, key.clone(), blobstore_bytes.clone()).await?;

        assert!(
            bs.is_present(ctx, &key).await?.assume_not_found_if_unsure(),
            "Blob should exist now"
        );
        Ok(())
    })
    .await
}

#[fbinit::test]
async fn overwrite(fb: FacebookInit) -> Result<(), Error> {
    test_chunking_methods(fb, PutBehaviour::Overwrite, |ctx, bs, _| async move {
        borrowed!(ctx);
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key = format!("manifoldblob_test_{}", suffix);

        let mut bytes_1 = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_1);
        let mut bytes_2 = [0u8; 32];
        thread_rng().fill_bytes(&mut bytes_2);

        let blobstore_bytes_1 = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_1));
        let blobstore_bytes_2 = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_2));

        // Write a fresh blob
        bs.put(ctx, key.clone(), blobstore_bytes_1.clone()).await?;
        // Overwrite it
        bs.put(ctx, key.clone(), blobstore_bytes_2.clone()).await?;

        assert_eq!(
            bs.get(ctx, &key).await?.map(|get| get.into_bytes()),
            Some(blobstore_bytes_2),
        );
        Ok(())
    })
    .await
}

#[fbinit::test]
async fn dedup(fb: FacebookInit) -> Result<(), Error> {
    test_chunking_methods(fb, DEFAULT_PUT_BEHAVIOUR, |ctx, bs, _| async move {
        borrowed!(ctx);
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key1 = format!("manifoldblob_test_{}", suffix);
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key2 = format!("manifoldblob_test_{}", suffix);

        let mut bytes_in = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

        assert!(
            !bs.is_present(ctx, &key1)
                .await?
                .assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        assert!(
            !bs.is_present(ctx, &key2)
                .await?
                .assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        // Write a fresh blob
        bs.put(ctx, key1.clone(), blobstore_bytes.clone()).await?;
        // Write it again under a different key
        bs.put(ctx, key2.clone(), blobstore_bytes.clone()).await?;

        // Reach inside the store and confirm it only stored the data once
        let data_store = bs.get_data_store();
        let row1 = data_store.get(&key1).await?.expect("Blob 1 not found");
        let row2 = data_store.get(&key2).await?.expect("Blob 2 not found");
        assert_eq!(row1.id, row2.id, "Chunk stored under different ids");
        assert_eq!(row1.count, row2.count, "Chunk count differs");
        assert_eq!(
            row1.chunking_method, row2.chunking_method,
            "Chunking method differs"
        );
        Ok(())
    })
    .await
}

#[fbinit::test]
async fn link(fb: FacebookInit) -> Result<(), Error> {
    test_chunking_methods(fb, DEFAULT_PUT_BEHAVIOUR, |ctx, bs, _| async move {
        borrowed!(ctx);
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key1 = format!("manifoldblob_test_{}", suffix);
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key2 = format!("manifoldblob_test_{}", suffix);

        let mut bytes_in = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

        assert!(
            !bs.is_present(ctx, &key1)
                .await?
                .assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        assert!(
            !bs.is_present(ctx, &key2)
                .await?
                .assume_not_found_if_unsure(),
            "Blob should not exist yet"
        );

        // Write a fresh blob
        bs.put(ctx, key1.clone(), blobstore_bytes.clone()).await?;
        // Link to a different key
        bs.copy(ctx, &key1, key2.clone()).await?;

        // Check that reads from the two keys match
        let bytes1 = bs.get(ctx, &key1).await?;
        let bytes2 = bs.get(ctx, &key2).await?;
        assert_eq!(
            bytes1.unwrap().as_raw_bytes(),
            bytes2.unwrap().as_raw_bytes()
        );

        // Reach inside the store and confirm it only stored the data once
        let data_store = bs.get_data_store();
        let row1 = data_store.get(&key1).await?.expect("Blob 1 not found");
        let row2 = data_store.get(&key2).await?.expect("Blob 2 not found");
        assert_eq!(row1.id, row2.id, "Chunk stored under different ids");
        assert_eq!(row1.count, row2.count, "Chunk count differs");
        assert_eq!(
            row1.chunking_method, row2.chunking_method,
            "Chunking method differs"
        );
        Ok(())
    })
    .await
}

#[fbinit::test]
async fn generations(fb: FacebookInit) -> Result<(), Error> {
    for auto_inline_puts in [true, false] {
        // Generate unique keys.
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key1 = format!("manifoldblob_test_{}", suffix);
        let suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let key2 = format!("manifoldblob_test_{}", suffix);

        let blobstore_bytes_inline = {
            let mut bytes_in = [0u8; MAX_INLINE_LEN as usize];
            thread_rng().fill_bytes(&mut bytes_in);
            BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in))
        };

        let blobstore_bytes = {
            let mut bytes_in = [0u8; 1024];
            thread_rng().fill_bytes(&mut bytes_in);
            BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in))
        };

        for blobstore_bytes in [blobstore_bytes_inline, blobstore_bytes] {
            let (test_source, config_store) = get_test_config_store();
            let bs = Sqlblob::with_sqlite_in_memory(
                DEFAULT_PUT_BEHAVIOUR,
                &config_store,
                auto_inline_puts,
                0, // no grace period for ctime updates,
            )?;
            let ctx = CoreContext::test_mock(fb);
            borrowed!(ctx);

            // Write a fresh blob
            bs.put_with_status(ctx, key1.clone(), blobstore_bytes.clone())
                .await?;

            let generations = bs.get_chunk_generations(&key1).await?;

            let value_len: u64 = blobstore_bytes.len().try_into()?;
            if !auto_inline_puts || value_len > MAX_INLINE_LEN {
                // Inspect, and determine that the generation number is present
                assert_eq!(
                    generations,
                    vec![Some(2)],
                    "Generation set to 2 {} {}",
                    auto_inline_puts,
                    value_len
                );
            } else {
                assert_eq!(generations, vec![], "No generations expected");
            }

            let mark_gen = set_test_generations(test_source.as_ref(), 4, 3, 0, INITIAL_VERSION + 1);

            config_store.force_update_configs();

            // Set the generation and confirm
            bs.set_generation(&key1, true, mark_gen).await?;
            let generations = bs.get_chunk_generations(&key1).await?;
            if value_len > MAX_INLINE_LEN {
                assert_eq!(generations, vec![Some(3)], "Generation set to 3");
            } else {
                // We should now have no generations as the set_generation(key, true) should have inlined
                assert_eq!(generations, vec![], "No generations expected");
            }

            // don't need to run these with variety of key lengths
            if value_len > MAX_INLINE_LEN {
                // Update via another key, confirm both have put generation
                set_test_generations(test_source.as_ref(), 5, 4, 2, INITIAL_VERSION + 2);
                config_store.force_update_configs();
                bs.put(ctx, key2.clone(), blobstore_bytes.clone()).await?;
                let generations = bs.get_chunk_generations(&key1).await?;
                assert_eq!(generations, vec![Some(5)], "key1 generation not updated");
                let generations = bs.get_chunk_generations(&key2).await?;
                assert_eq!(generations, vec![Some(5)], "key2 generation not updated");

                // Now update via the route GC uses, confirm it updates nicely and doesn't leap to
                // the wrong version.
                let mark_gen =
                    set_test_generations(test_source.as_ref(), 999, 10, 3, INITIAL_VERSION + 3);
                config_store.force_update_configs();
                bs.set_generation(&key1, true, mark_gen).await?;
                let generations = bs.get_chunk_generations(&key1).await?;
                assert_eq!(generations, vec![Some(10)], "key1 generation not updated");
                let generations = bs.get_chunk_generations(&key2).await?;
                assert_eq!(generations, vec![Some(10)], "key2 generation not updated");
            }
        }
    }
    Ok(())
}
