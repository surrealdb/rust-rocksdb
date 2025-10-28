mod util;

use rocksdb::{Env, Options, SstFileManager, DB};
use util::DBPath;

#[test]
fn test_sst_file_manager_new() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env);
    assert!(sst_file_manager.is_ok());
}

#[test]
fn test_sst_file_manager_set_max_allowed_space_usage() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Should not panic
    sst_file_manager.set_max_allowed_space_usage(1024 * 1024 * 1024); // 1GB

    // Setting to 0 should disable the limit
    sst_file_manager.set_max_allowed_space_usage(0);
}

#[test]
fn test_sst_file_manager_set_compaction_buffer_size() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Should not panic
    sst_file_manager.set_compaction_buffer_size(64 * 1024 * 1024); // 64MB
}

#[test]
fn test_sst_file_manager_is_max_allowed_space_reached() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Initially should not be reached
    assert!(!sst_file_manager.is_max_allowed_space_reached());

    // Set a very small limit
    sst_file_manager.set_max_allowed_space_usage(1);

    // Without actual SST files, should still be false
    assert!(!sst_file_manager.is_max_allowed_space_reached());
}

#[test]
fn test_sst_file_manager_is_max_allowed_space_reached_including_compactions() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Initially should not be reached
    assert!(!sst_file_manager.is_max_allowed_space_reached_including_compactions());

    // Set buffer size
    sst_file_manager.set_compaction_buffer_size(100);

    // Without actual SST files, should still be false
    assert!(!sst_file_manager.is_max_allowed_space_reached_including_compactions());
}

#[test]
fn test_sst_file_manager_get_total_size() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Without tracking any files, total size should be 0
    let total_size = sst_file_manager.get_total_size();
    assert_eq!(total_size, 0);
}

#[test]
fn test_sst_file_manager_delete_rate() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Test setting and getting delete rate
    sst_file_manager.set_delete_rate_bytes_per_second(64 * 1024 * 1024); // 64MB/s
    let rate = sst_file_manager.get_delete_rate_bytes_per_second();
    assert_eq!(rate, 64 * 1024 * 1024);

    // Test setting to 0 (unlimited)
    sst_file_manager.set_delete_rate_bytes_per_second(0);
    let rate = sst_file_manager.get_delete_rate_bytes_per_second();
    assert_eq!(rate, 0);
}

#[test]
fn test_sst_file_manager_trash_ratio() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Test setting and getting trash ratio
    sst_file_manager.set_max_trash_db_ratio(0.25);
    let ratio = sst_file_manager.get_max_trash_db_ratio();
    assert!((ratio - 0.25).abs() < 0.001);

    // Test another value
    sst_file_manager.set_max_trash_db_ratio(0.5);
    let ratio = sst_file_manager.get_max_trash_db_ratio();
    assert!((ratio - 0.5).abs() < 0.001);
}

#[test]
fn test_sst_file_manager_get_total_trash_size() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Without any trash files, should be 0
    let trash_size = sst_file_manager.get_total_trash_size();
    assert_eq!(trash_size, 0);
}

#[test]
fn test_sst_file_manager_with_options() {
    let path = DBPath::new("_rust_rocksdb_sst_file_manager_with_options_test");

    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Configure the manager
    sst_file_manager.set_delete_rate_bytes_per_second(64 * 1024 * 1024);
    sst_file_manager.set_max_trash_db_ratio(0.25);

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_sst_file_manager(&sst_file_manager);

    // Open database with the SstFileManager
    let db = DB::open(&opts, &path).unwrap();

    // Insert some data
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    // Flush to create SST files
    db.flush().unwrap();

    // Verify data
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));

    drop(db);
    let _ = DB::destroy(&Options::default(), &path);
}

#[test]
fn test_sst_file_manager_with_data() {
    let path = DBPath::new("_rust_rocksdb_sst_file_manager_with_data_test");

    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_env(&env);
    opts.set_sst_file_manager(&sst_file_manager);

    let db = DB::open(&opts, &path).unwrap();

    // Insert data to create SST files
    for i in 0..1000 {
        let key = format!("key{:04}", i);
        let value = format!("value{:04}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Flush to ensure SST files are created
    db.flush().unwrap();

    // Check that total size is greater than 0 after flushing
    let total_size = sst_file_manager.get_total_size();
    assert!(
        total_size > 0,
        "Total size should be greater than 0 after flushing data"
    );

    drop(db);
    let _ = DB::destroy(&Options::default(), &path);
}

#[test]
fn test_sst_file_manager_clone() {
    let env = Env::new().unwrap();
    let sst_file_manager = SstFileManager::new(&env).unwrap();

    // Set some values
    sst_file_manager.set_delete_rate_bytes_per_second(64 * 1024 * 1024);
    sst_file_manager.set_max_trash_db_ratio(0.25);

    // Clone the manager
    let cloned = sst_file_manager.clone();

    // Both should have the same values
    assert_eq!(
        sst_file_manager.get_delete_rate_bytes_per_second(),
        cloned.get_delete_rate_bytes_per_second()
    );

    let ratio1 = sst_file_manager.get_max_trash_db_ratio();
    let ratio2 = cloned.get_max_trash_db_ratio();
    assert!((ratio1 - ratio2).abs() < 0.001);
}

#[test]
fn test_sst_file_manager_thread_safe() {
    use std::sync::Arc;
    use std::thread;

    let env = Env::new().unwrap();
    let sst_file_manager = Arc::new(SstFileManager::new(&env).unwrap());

    let manager1 = Arc::clone(&sst_file_manager);
    let manager2 = Arc::clone(&sst_file_manager);

    let handle1 = thread::spawn(move || {
        manager1.set_delete_rate_bytes_per_second(64 * 1024 * 1024);
        manager1.get_total_size()
    });

    let handle2 = thread::spawn(move || {
        manager2.set_max_trash_db_ratio(0.25);
        manager2.get_total_trash_size()
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}
