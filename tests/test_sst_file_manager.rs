use rocksdb::{DB, Env, Options, SstFileManager};

#[test]
fn test_sst_file_manager_create() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    assert_eq!(sfm.get_total_size(), 0);
}

#[test]
fn test_sst_file_manager_max_allowed_space() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_max_allowed_space_usage(1024 * 1024);
    assert!(!sfm.is_max_allowed_space_reached());
    assert!(!sfm.is_max_allowed_space_reached_including_compactions());
}

#[test]
fn test_sst_file_manager_compaction_buffer_size() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_compaction_buffer_size(512 * 1024);
}

#[test]
fn test_sst_file_manager_delete_rate() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_delete_rate_bytes_per_second(1024 * 1024);
    assert_eq!(sfm.get_delete_rate_bytes_per_second(), 1024 * 1024);
}

#[test]
fn test_sst_file_manager_trash_ratio() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_max_trash_db_ratio(0.5);
    assert!((sfm.get_max_trash_db_ratio() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn test_sst_file_manager_total_trash_size() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    assert_eq!(sfm.get_total_trash_size(), 0);
}

#[test]
fn test_sst_file_manager_with_db() {
    let tempdir = tempfile::Builder::new()
        .prefix("_rust_rocksdb_sst_file_manager_test")
        .tempdir()
        .unwrap();
    let path = tempdir.path();

    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_max_allowed_space_usage(100 * 1024 * 1024);
    sfm.set_delete_rate_bytes_per_second(1024 * 1024);

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_sst_file_manager(&sfm);

    {
        let db = DB::open(&opts, path).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        assert_eq!(db.get(b"key1").unwrap().unwrap(), b"value1");
    }
    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_sst_file_manager_clone() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();
    sfm.set_delete_rate_bytes_per_second(2048);

    let sfm2 = sfm.clone();
    assert_eq!(sfm2.get_delete_rate_bytes_per_second(), 2048);
}

#[test]
fn test_sst_file_manager_send_sync() {
    let env = Env::new().unwrap();
    let sfm = SstFileManager::new(&env).unwrap();

    std::thread::spawn(move || {
        sfm.set_delete_rate_bytes_per_second(4096);
        assert_eq!(sfm.get_delete_rate_bytes_per_second(), 4096);
    })
    .join()
    .unwrap();
}
