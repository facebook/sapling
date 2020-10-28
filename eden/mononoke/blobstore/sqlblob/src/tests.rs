/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use blobstore::DEFAULT_PUT_BEHAVIOUR;
use bytes::Bytes;
use fbinit::FacebookInit;
use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};
use std::time::Duration;

const UPDATE_WAIT_TIME: Duration = Duration::from_millis(3);

#[fbinit::compat_test]
async fn read_write(fb: FacebookInit) {
    let (_, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key = format!("manifoldblob_test_{}", suffix);

    let bs =
        Arc::new(Sqlblob::with_sqlite_in_memory(DEFAULT_PUT_BEHAVIOUR, &config_store).unwrap());

    let mut bytes_in = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_in);

    let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

    assert!(
        !bs.is_present(ctx.clone(), key.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    // Write a fresh blob
    bs.put(ctx.clone(), key.clone(), blobstore_bytes)
        .await
        .unwrap();
    // Read back and verify
    let bytes_out = bs.get(ctx.clone(), key.clone()).await.unwrap();
    assert_eq!(&bytes_in.to_vec(), bytes_out.unwrap().as_raw_bytes());

    assert!(
        bs.is_present(ctx.clone(), key.clone()).await.unwrap(),
        "Blob should exist now"
    );
}

#[fbinit::compat_test]
async fn double_put(fb: FacebookInit) {
    let (_, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key = format!("manifoldblob_test_{}", suffix);

    let bs =
        Arc::new(Sqlblob::with_sqlite_in_memory(DEFAULT_PUT_BEHAVIOUR, &config_store).unwrap());

    let mut bytes_in = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_in);

    let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

    assert!(
        !bs.is_present(ctx.clone(), key.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    // Write a fresh blob
    bs.put(ctx.clone(), key.clone(), blobstore_bytes.clone())
        .await
        .unwrap();
    // Write it again
    bs.put(ctx.clone(), key.clone(), blobstore_bytes.clone())
        .await
        .unwrap();

    assert!(
        bs.is_present(ctx.clone(), key.clone()).await.unwrap(),
        "Blob should exist now"
    );
}

#[fbinit::compat_test]
async fn overwrite(fb: FacebookInit) -> Result<()> {
    let (_, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key = format!("manifoldblob_test_{}", suffix);

    let bs =
        Arc::new(Sqlblob::with_sqlite_in_memory(PutBehaviour::Overwrite, &config_store).unwrap());

    let mut bytes_1 = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_1);
    let mut bytes_2 = [0u8; 32];
    thread_rng().fill_bytes(&mut bytes_2);

    let blobstore_bytes_1 = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_1));
    let blobstore_bytes_2 = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_2));

    // Write a fresh blob
    bs.put(ctx.clone(), key.clone(), blobstore_bytes_1.clone())
        .await?;
    // Overwrite it
    bs.put(ctx.clone(), key.clone(), blobstore_bytes_2.clone())
        .await?;

    assert_eq!(
        bs.get(ctx.clone(), key.clone())
            .await?
            .map(|get| get.into_bytes()),
        Some(blobstore_bytes_2),
    );
    Ok(())
}

#[fbinit::compat_test]
async fn dedup(fb: FacebookInit) {
    let (_, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key1 = format!("manifoldblob_test_{}", suffix);
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key2 = format!("manifoldblob_test_{}", suffix);

    let bs =
        Arc::new(Sqlblob::with_sqlite_in_memory(DEFAULT_PUT_BEHAVIOUR, &config_store).unwrap());

    let mut bytes_in = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_in);

    let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

    assert!(
        !bs.is_present(ctx.clone(), key1.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    assert!(
        !bs.is_present(ctx.clone(), key2.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    // Write a fresh blob
    bs.put(ctx.clone(), key1.clone(), blobstore_bytes.clone())
        .await
        .unwrap();
    // Write it again under a different key
    bs.put(ctx.clone(), key2.clone(), blobstore_bytes.clone())
        .await
        .unwrap();

    // Reach inside the store and confirm it only stored the data once
    let data_store = bs.as_inner().get_data_store();
    let row1 = data_store
        .get(&key1)
        .await
        .unwrap()
        .expect("Blob 1 not found");
    let row2 = data_store
        .get(&key2)
        .await
        .unwrap()
        .expect("Blob 2 not found");
    assert_eq!(row1.id, row2.id, "Chunk stored under different ids");
    assert_eq!(row1.count, row2.count, "Chunk count differs");
    assert_eq!(
        row1.chunking_method, row2.chunking_method,
        "Chunking method differs"
    );
}

#[fbinit::compat_test]
async fn link(fb: FacebookInit) {
    let (_, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key1 = format!("manifoldblob_test_{}", suffix);
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key2 = format!("manifoldblob_test_{}", suffix);

    let bs =
        Arc::new(Sqlblob::with_sqlite_in_memory(DEFAULT_PUT_BEHAVIOUR, &config_store).unwrap());

    let mut bytes_in = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_in);

    let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

    assert!(
        !bs.is_present(ctx.clone(), key1.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    assert!(
        !bs.is_present(ctx.clone(), key2.clone()).await.unwrap(),
        "Blob should not exist yet"
    );

    // Write a fresh blob
    bs.put(ctx.clone(), key1.clone(), blobstore_bytes.clone())
        .await
        .unwrap();
    // Link to a different key
    bs.link(ctx.clone(), key1.clone(), key2.clone())
        .await
        .unwrap();

    // Check that reads from the two keys match
    let bytes1 = bs.get(ctx.clone(), key1.clone()).await.unwrap();
    let bytes2 = bs.get(ctx.clone(), key2.clone()).await.unwrap();
    assert_eq!(
        bytes1.unwrap().as_raw_bytes(),
        bytes2.unwrap().as_raw_bytes()
    );

    // Reach inside the store and confirm it only stored the data once
    let data_store = bs.as_inner().get_data_store();
    let row1 = data_store
        .get(&key1)
        .await
        .unwrap()
        .expect("Blob 1 not found");
    let row2 = data_store
        .get(&key2)
        .await
        .unwrap()
        .expect("Blob 2 not found");
    assert_eq!(row1.id, row2.id, "Chunk stored under different ids");
    assert_eq!(row1.count, row2.count, "Chunk count differs");
    assert_eq!(
        row1.chunking_method, row2.chunking_method,
        "Chunking method differs"
    );
}

#[fbinit::compat_test]
async fn generations(fb: FacebookInit) -> Result<()> {
    let (test_source, config_store) = get_test_config_store();
    let ctx = CoreContext::test_mock(fb);
    // Generate unique keys.
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key1 = format!("manifoldblob_test_{}", suffix);
    let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
    let key2 = format!("manifoldblob_test_{}", suffix);

    let bs = Arc::new(Sqlblob::with_sqlite_in_memory(
        DEFAULT_PUT_BEHAVIOUR,
        &config_store,
    )?);

    let mut bytes_in = [0u8; 64];
    thread_rng().fill_bytes(&mut bytes_in);

    let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

    // Write a fresh blob
    bs.put(ctx.clone(), key1.clone(), blobstore_bytes.clone())
        .await?;

    // Inspect, and determine that the generation number is missing
    let generations = bs.as_inner().get_chunk_generations(&key1).await?;
    assert_eq!(generations, vec![None], "Generation appeared unexpectedly");

    // Set the generation and confirm
    bs.as_inner().set_generation(&key1).await?;
    let generations = bs.as_inner().get_chunk_generations(&key1).await?;
    assert_eq!(generations, vec![Some(2)], "Generation set to 2");

    // Update via another key, confirm both have put generation
    set_test_generations(test_source.as_ref(), 5, 4, 2, INITIAL_VERSION + 1);
    tokio::time::delay_for(UPDATE_WAIT_TIME).await;
    bs.put(ctx.clone(), key2.clone(), blobstore_bytes.clone())
        .await?;
    let generations = bs.as_inner().get_chunk_generations(&key1).await?;
    assert_eq!(generations, vec![Some(5)], "key1 generation not updated");
    let generations = bs.as_inner().get_chunk_generations(&key2).await?;
    assert_eq!(generations, vec![Some(5)], "key2 generation not updated");

    // Now update via the route GC uses, confirm it updates nicely and doesn't leap to
    // the wrong version.
    set_test_generations(test_source.as_ref(), 999, 10, 3, INITIAL_VERSION + 2);
    tokio::time::delay_for(UPDATE_WAIT_TIME).await;
    bs.as_inner().set_generation(&key1).await?;
    let generations = bs.as_inner().get_chunk_generations(&key1).await?;
    assert_eq!(generations, vec![Some(10)], "key1 generation not updated");
    let generations = bs.as_inner().get_chunk_generations(&key2).await?;
    assert_eq!(generations, vec![Some(10)], "key2 generation not updated");
    Ok(())
}
