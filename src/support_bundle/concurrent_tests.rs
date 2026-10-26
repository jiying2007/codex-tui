    #[test]
    fn concurrent_collectors_cannot_overwrite_the_same_destination() {
        use std::sync::{Arc, Barrier};

        const COLLECTORS: usize = 12;
        let root = tempdir().expect("tempdir");
        let destination = root.path().join("nested").join("bundle");
        let start = Arc::new(Barrier::new(COLLECTORS));
        let handles: Vec<_> = (0..COLLECTORS)
            .map(|_| {
                let start = Arc::clone(&start);
                let destination = destination.clone();
                std::thread::spawn(move || {
                    start.wait();
                    write_bundle(&destination, &fixture()).is_ok()
                })
            })
            .collect();

        let successes = handles
            .into_iter()
            .filter(|handle| handle.join().expect("collector thread"))
            .count();
        assert_eq!(successes, 1, "only one collector may claim an output path");
        assert!(destination.join("snapshot.json").is_file());
        assert!(destination.join("manifest.json").is_file());
        assert!(destination.join("PRIVACY.txt").is_file());
        assert!(
            write_bundle(&destination, &fixture()).is_err(),
            "an existing bundle must never be overwritten"
        );
    }

