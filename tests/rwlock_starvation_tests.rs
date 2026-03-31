//! Tests demonstrating RwLock writer starvation vs DashMap lock-free access
//!
//! These tests demonstrate the problem that Phase 9 fixes:
//! - RwLock<HashMap> can cause writer starvation under high read load
//! - DashMap provides lock-free reads and striped writes, eliminating starvation

use dashmap::DashMap;
use parking_lot_rt::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// This test demonstrates that RwLock writers CAN be delayed by continuous readers.
///
/// Under heavy read load, a writer trying to acquire a write lock may be blocked
/// while readers continue to acquire read locks. This is because RwLock typically
/// prioritizes readers (to increase throughput) or gives them equal priority.
///
/// In a real-time system (like this middleware with RT scheduling), this can cause:
/// - Priority inversion: High-priority writer blocked by lower-priority readers
/// - RT scheduling violations: Worker missing its scheduling deadline
/// - Cascading delays: Other workers waiting on the blocked writer
#[test]
fn test_rwlock_writer_can_be_delayed_by_readers() {
    let map: Arc<RwLock<HashMap<String, i32>>> = Arc::new(RwLock::new(HashMap::new()));
    map.write().insert("key".to_string(), 0);

    let map_clone = map.clone();
    // Spawn a reader thread that continuously reads for a period
    let reader_handle = thread::spawn(move || {
        for _ in 0..1000 {
            let _ = map_clone.read().get(&"key".to_string());
            thread::sleep(Duration::from_micros(10));
        }
    });

    // Give readers a moment to start
    thread::sleep(Duration::from_millis(5));

    let start = std::time::Instant::now();
    map.write().insert("key".to_string(), 1);
    let write_time = start.elapsed();

    reader_handle.join().unwrap();

    // In ideal conditions, write should complete quickly (< 50ms)
    // But under read pressure, RwLock may delay the writer
    println!("RwLock write completed in {:?}", write_time);

    // This test doesn't assert specific timing because behavior varies
    // The point is to demonstrate the MECHANISM, not guarantee specific delays
}

/// This test demonstrates that DashMap does NOT suffer from writer starvation.
///
/// DashMap uses a sharded architecture:
/// - Multiple internal shards (each with its own RwLock)
/// - Lock-free reads (no blocking on shard locks)
/// - Writers only block on the specific shard they're modifying
///
/// This means:
/// - A writer to shard N doesn't block readers from shard M
/// - High read load on one key doesn't block writes to another key  
/// - RT workers can update device status without being blocked by HTTP reads
#[test]
fn test_dashmap_no_writer_starvation() {
    let map: Arc<DashMap<String, i32>> = Arc::new(DashMap::new());
    map.insert("key".to_string(), 0);

    let map_clone = map.clone();
    // Spawn a reader thread that continuously reads for a period
    let reader_handle = thread::spawn(move || {
        for _ in 0..1000 {
            let _ = map_clone.get(&"key".to_string());
            thread::sleep(Duration::from_micros(10));
        }
    });

    // Give readers a moment to start
    thread::sleep(Duration::from_millis(5));

    let start = std::time::Instant::now();
    map.insert("key".to_string(), 1);
    let write_time = start.elapsed();

    reader_handle.join().unwrap();

    println!("DashMap write completed in {:?}", write_time);

    // DashMap write should complete very quickly (< 5ms typically)
    // because it doesn't need to wait for all readers to finish
    assert!(
        write_time < Duration::from_millis(50),
        "DashMap write should not be significantly delayed by readers"
    );
}

/// Test demonstrating the sharded nature of DashMap.
///
/// With multiple keys, DashMap distributes them across shards.
/// This means concurrent writes to different keys don't block each other.
#[test]
fn test_dashmap_concurrent_writes_different_keys() {
    let map: Arc<DashMap<String, i32>> = Arc::new(DashMap::new());

    // Insert multiple keys (likely distributed across different shards)
    for i in 0..16 {
        map.insert(format!("key-{}", i), 0);
    }

    let map_clone1 = map.clone();
    let map_clone2 = map.clone();

    // Two threads writing to different keys simultaneously
    let writer1 = thread::spawn(move || {
        for i in 0..100 {
            map_clone1.insert(format!("key-{}", i % 16), i);
        }
    });

    let writer2 = thread::spawn(move || {
        for i in 0..100 {
            map_clone2.insert(format!("key-{}", (i + 8) % 16), i);
        }
    });

    writer1.join().unwrap();
    writer2.join().unwrap();

    // Both writers should have completed without significant delay
    // With RwLock, one writer would have blocked the other
}

/// Test demonstrating that DashMap reads don't block writes.
///
/// This is the key property for our middleware:
/// - HTTP Worker (async, many concurrent reads) doesn't block
/// - HeartbeatWorker (RT priority 70, writes device status)
/// - Both can operate without blocking each other
#[test]
fn test_dashmap_reads_dont_block_writes() {
    let map: Arc<DashMap<String, i32>> = Arc::new(DashMap::new());
    map.insert("device-1".to_string(), 0);

    let map_clone = map.clone();

    // Simulate HTTP Worker: many concurrent readers
    let readers: Vec<_> = (0..10)
        .map(|_| {
            let map_ref = map_clone.clone();
            thread::spawn(move || {
                for _ in 0..100 {
                    // Multiple concurrent reads
                    let _ = map_ref.get(&"device-1".to_string());
                    thread::sleep(Duration::from_micros(100));
                }
            })
        })
        .collect();

    // Simulate HeartbeatWorker: periodic writes
    let start = std::time::Instant::now();
    for i in 0..10 {
        map.insert("device-1".to_string(), i);
        thread::sleep(Duration::from_millis(10));
    }
    let write_total_time = start.elapsed();

    // Wait for all readers to finish
    for reader in readers {
        reader.join().unwrap();
    }

    println!(
        "DashMap: 10 writes completed in {:?} with 10 concurrent readers",
        write_total_time
    );

    // Total time should be close to 100ms (10 writes × 10ms interval)
    // Not significantly delayed by readers
    assert!(
        write_total_time < Duration::from_millis(150),
        "Writes should not be significantly delayed by concurrent reads"
    );
}

/// Test comparing RwLock vs DashMap under realistic middleware load pattern.
///
/// Pattern: Multiple readers (HTTP API) + single writer (HeartbeatWorker)
#[test]
fn test_realistic_load_pattern_comparison() {
    // RwLock scenario
    let rwlock_map: Arc<RwLock<HashMap<String, i32>>> = Arc::new(RwLock::new(HashMap::new()));
    rwlock_map.write().insert("plc-1".to_string(), 1);

    let rwlock_clone = rwlock_map.clone();
    let rwlock_readers: Vec<_> = (0..5)
        .map(|_| {
            let map_ref = rwlock_clone.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    let _ = map_ref.read().get(&"plc-1".to_string());
                    thread::sleep(Duration::from_micros(50));
                }
            })
        })
        .collect();

    let rwlock_start = std::time::Instant::now();
    for i in 0..5 {
        rwlock_map.write().insert("plc-1".to_string(), i);
        thread::sleep(Duration::from_millis(5));
    }
    let rwlock_write_time = rwlock_start.elapsed();

    for reader in rwlock_readers {
        reader.join().unwrap();
    }

    // DashMap scenario
    let dashmap: Arc<DashMap<String, i32>> = Arc::new(DashMap::new());
    dashmap.insert("plc-1".to_string(), 1);

    let dashmap_clone = dashmap.clone();
    let dashmap_readers: Vec<_> = (0..5)
        .map(|_| {
            let map_ref = dashmap_clone.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    let _ = map_ref.get(&"plc-1".to_string());
                    thread::sleep(Duration::from_micros(50));
                }
            })
        })
        .collect();

    let dashmap_start = std::time::Instant::now();
    for i in 0..5 {
        dashmap.insert("plc-1".to_string(), i);
        thread::sleep(Duration::from_millis(5));
    }
    let dashmap_write_time = dashmap_start.elapsed();

    for reader in dashmap_readers {
        reader.join().unwrap();
    }

    println!("RwLock write time with 5 readers: {:?}", rwlock_write_time);
    println!(
        "DashMap write time with 5 readers: {:?}",
        dashmap_write_time
    );

    // DashMap should generally be faster or at least not slower
    // Note: This is a demonstration, not a strict performance assertion
    // Real performance difference is more pronounced under heavier load
}
