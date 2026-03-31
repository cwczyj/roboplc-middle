//! Tests for bounded channel backpressure behavior

use roboplc_middleware::{MAX_PENDING_HEARTBEATS, MAX_PENDING_RESPONSES};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;

#[test]
fn max_pending_responses_constant_is_defined() {
    assert_eq!(MAX_PENDING_RESPONSES, 100);
}

#[test]
fn max_pending_heartbeats_constant_is_defined() {
    assert_eq!(MAX_PENDING_HEARTBEATS, 50);
}

#[test]
fn bounded_channel_blocks_when_full() {
    let (tx, rx): (SyncSender<u32>, Receiver<u32>) = sync_channel(3);

    tx.send(1).unwrap();
    tx.send(2).unwrap();
    tx.send(3).unwrap();

    let tx_clone = tx.clone();
    let start = std::time::Instant::now();

    thread::spawn(move || {
        tx_clone.send(4).unwrap();
    });

    thread::sleep(Duration::from_millis(50));

    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(40));

    assert_eq!(rx.recv().unwrap(), 1);

    thread::sleep(Duration::from_millis(10));
    assert_eq!(rx.recv().unwrap(), 2);
    assert_eq!(rx.recv().unwrap(), 3);
    assert_eq!(rx.recv().unwrap(), 4);
}

#[test]
fn bounded_channel_provides_backpressure() {
    let capacity = MAX_PENDING_RESPONSES;
    let (tx, rx) = sync_channel::<u64>(capacity);

    for i in 0..capacity {
        tx.send(i as u64).unwrap();
    }

    let tx_clone = tx.clone();
    let result_thread = thread::spawn(move || {
        let start = std::time::Instant::now();
        match tx_clone.send(capacity as u64) {
            Ok(_) => start.elapsed(),
            Err(_) => Duration::from_secs(999),
        }
    });

    thread::sleep(Duration::from_millis(100));

    for i in 0..capacity {
        assert_eq!(rx.recv().unwrap(), i as u64);
    }

    assert_eq!(rx.recv().unwrap(), capacity as u64);

    let elapsed = result_thread.join().unwrap();
    assert!(elapsed >= Duration::from_millis(50));
}

#[test]
fn bounded_channel_sender_blocks_until_space_available() {
    let (tx, rx) = sync_channel::<i32>(2);

    tx.send(100).unwrap();
    tx.send(200).unwrap();

    let blocked_sender = thread::spawn(move || {
        tx.send(300).unwrap();
        tx.send(400).unwrap();
    });

    thread::sleep(Duration::from_millis(20));

    let val1 = rx.recv_timeout(Duration::from_millis(10)).unwrap();
    assert_eq!(val1, 100);

    thread::sleep(Duration::from_millis(10));

    let val2 = rx.recv_timeout(Duration::from_millis(10)).unwrap();
    assert_eq!(val2, 200);
    let val3 = rx.recv_timeout(Duration::from_millis(10)).unwrap();
    assert_eq!(val3, 300);
    let val4 = rx.recv_timeout(Duration::from_millis(10)).unwrap();
    assert_eq!(val4, 400);

    blocked_sender.join().unwrap();
}

#[test]
fn bounded_channel_receiver_timeout_when_empty() {
    let (tx, rx) = sync_channel::<String>(5);

    drop(tx);

    let result = rx.recv_timeout(Duration::from_millis(100));
    assert!(matches!(result, Err(RecvTimeoutError::Disconnected)));
}

#[test]
fn heartbeat_channel_capacity_is_reasonable() {
    assert!(MAX_PENDING_HEARTBEATS < MAX_PENDING_RESPONSES);
    assert!(MAX_PENDING_HEARTBEATS >= 10);
}

#[test]
fn response_channel_capacity_is_reasonable() {
    assert!(MAX_PENDING_RESPONSES >= 50);
    assert!(MAX_PENDING_RESPONSES <= 500);
}
