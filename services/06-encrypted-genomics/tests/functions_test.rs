#![allow(dead_code)]

include!("../src/functions.rs");

struct TestKeys {
    client_key: tfhe::ClientKey,
    public_key: tfhe::CompactPublicKey,
    public_key_b64: String,
    server_key: tfhe::ServerKey,
    server_key_b64: String,
}

static TEST_KEYS: std::sync::OnceLock<TestKeys> = std::sync::OnceLock::new();
static TEST_SERVER_KEY_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

fn test_keys() -> &'static TestKeys {
    TEST_KEYS.get_or_init(|| {
        let config = tfhe::ConfigBuilder::default().build();
        let (client_key, server_key) = tfhe::generate_keys(config);
        let public_key = tfhe::CompactPublicKey::new(&client_key);
        let compressed_server_key = tfhe::CompressedServerKey::new(&client_key);

        TestKeys {
            public_key_b64: serialize_test_value(&public_key),
            server_key_b64: serialize_test_value(&compressed_server_key),
            client_key,
            public_key,
            server_key,
        }
    })
}

fn serialize_test_value<T: serde::Serialize>(value: &T) -> String {
    BASE64.encode(bincode::serialize(value).unwrap())
}

fn install_test_server_key() {
    let _guard = TEST_SERVER_KEY_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap();
    let keys = test_keys();

    rayon::broadcast(|_| tfhe::set_server_key(keys.server_key.clone()));
    tfhe::set_server_key(keys.server_key.clone());
}

fn trivial_values(values: &[u8]) -> Vec<FheUint8> {
    install_test_server_key();

    values
        .iter()
        .copied()
        .map(FheUint8::encrypt_trivial)
        .collect()
}

fn clear_value(value: &FheUint8) -> u8 {
    install_test_server_key();

    value.decrypt(&test_keys().client_key)
}

fn clear_values(values: &[FheUint8]) -> Vec<u8> {
    values.iter().map(clear_value).collect()
}

fn encrypted_request_bases(values: &[u8]) -> Vec<String> {
    install_test_server_key();

    let encrypted = encrypt_clear_values(values, &test_keys().public_key).unwrap();
    serialize_fhe_items(&encrypted).unwrap()
}

fn clear_serialized_values(values: &[String]) -> Vec<u8> {
    values
        .iter()
        .map(|value| clear_value(&deserialize_fhe_item(value).unwrap()))
        .collect()
}

fn session_keys() -> SessionKeys {
    let keys = test_keys();

    SessionKeys {
        public_key: keys.public_key_b64.clone(),
        server_key: keys.server_key_b64.clone(),
    }
}

fn expected_hamming_windows(word: &[u8], pattern: &[u8]) -> Vec<u8> {
    word.windows(pattern.len())
        .map(|window| {
            window
                .iter()
                .zip(pattern.iter())
                .filter(|(left, right)| left != right)
                .count() as u8
        })
        .collect()
}

fn expected_levenshtein(left: &[u8], right: &[u8]) -> u8 {
    let mut dp = vec![vec![0u8; right.len() + 1]; left.len() + 1];

    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i as u8;
    }
    for j in 0..=right.len() {
        dp[0][j] = j as u8;
    }

    for i in 1..=left.len() {
        for j in 1..=right.len() {
            let cost = u8::from(left[i - 1] != right[j - 1]);
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[left.len()][right.len()]
}

fn risk_pattern(id: i32, sequence: &str) -> RiskPattern {
    RiskPattern {
        id,
        sequence: sequence.to_string(),
    }
}

fn encrypted_pattern(id: i32, sequence: &str) -> EncryptedRiskPattern {
    EncryptedRiskPattern {
        pattern: risk_pattern(id, sequence),
        encrypted_bases: trivial_values(&encode_dna(sequence).unwrap()),
    }
}

fn process_request(values: &[u8]) -> ProcessRequest {
    ProcessRequest {
        encrypted_sequence: None,
        encrypted_bases: Some(encrypted_request_bases(values)),
        server_key: None,
        public_key: None,
        session_id: None,
        pattern_id: None,
    }
}

fn compare_database_request(values: &[u8]) -> CompareDatabaseRequest {
    CompareDatabaseRequest {
        encrypted_sequence: None,
        encrypted_bases: Some(encrypted_request_bases(values)),
        server_key: None,
        public_key: None,
        session_id: None,
        pattern_id: None,
    }
}

fn compare_session_sequences_request(
    session_id: &str,
    values: &[u8],
) -> CompareSessionSequencesRequest {
    CompareSessionSequencesRequest {
        session_id: session_id.to_string(),
        encrypted_sequence: None,
        encrypted_bases: Some(encrypted_request_bases(values)),
    }
}

fn stored_session_sequence(id: i32, session_id: &str, values: &[u8]) -> StoredSessionSequence {
    let encrypted_bases = encrypted_request_bases(values);

    StoredSessionSequence {
        info: SessionSequenceInfo {
            id,
            session_id: session_id.to_string(),
            original_length: values.len(),
            created_at: "test".to_string(),
            encrypted_bases: encrypted_bases.clone(),
        },
        encrypted_bases,
    }
}

#[test]
fn parse_pattern_accepts_dna_and_numeric_codes() {
    assert_eq!(parse_pattern("ATCG").unwrap(), vec![0, 1, 2, 3]);
    assert_eq!(parse_pattern("0 1 2 3").unwrap(), vec![0, 1, 2, 3]);
}

#[test]
fn parse_pattern_rejects_empty_illegal_base_and_illegal_digit() {
    assert!(parse_pattern("   ").is_err());
    assert!(parse_pattern("AX").is_err());
    assert!(parse_pattern("04").is_err());
}

#[test]
fn hamming_distance_matches_for_one_to_three_base_patterns() {
    let word = trivial_values(&[0, 1, 2]);
    assert_eq!(
        clear_value(&homomorphic_hamming_distance(&word[..1], &[0])),
        0
    );
    assert_eq!(
        clear_value(&homomorphic_hamming_distance(&word[..2], &[0, 2])),
        1
    );
    assert_eq!(
        clear_value(&homomorphic_hamming_distance(&word, &[3, 2, 1])),
        3
    );
}

#[test]
fn encrypted_hamming_distance_matches_for_one_to_three_base_patterns() {
    let word = trivial_values(&[0, 1, 2]);
    assert_eq!(
        clear_value(&homomorphic_hamming_distance_encrypted(
            &word[..1],
            &trivial_values(&[0])
        )),
        0
    );
    assert_eq!(
        clear_value(&homomorphic_hamming_distance_encrypted(
            &word[..2],
            &trivial_values(&[1, 1])
        )),
        1
    );
    assert_eq!(
        clear_value(&homomorphic_hamming_distance_encrypted(
            &word,
            &trivial_values(&[3, 2, 1])
        )),
        3
    );
}

#[test]
fn hamming_sliding_window_checks_fifteen_base_word_with_three_base_pattern() {
    let word = [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2];
    let pattern = [0, 1, 2];
    let encrypted_word = trivial_values(&word);

    let distances = homomorphic_sliding_window(&encrypted_word, &pattern).unwrap();

    assert_eq!(
        clear_values(&distances),
        expected_hamming_windows(&word, &pattern)
    );
    assert_eq!(distances.len(), 13);
}

#[test]
fn encrypted_hamming_sliding_window_matches_plain_expected_windows() {
    let word = [0, 1, 2, 3, 0];
    let pattern = [0, 1, 3];
    let encrypted_word = trivial_values(&word);
    let encrypted_pattern = trivial_values(&pattern);

    let distances =
        homomorphic_sliding_window_encrypted(&encrypted_word, &encrypted_pattern).unwrap();

    assert_eq!(
        clear_values(&distances),
        expected_hamming_windows(&word, &pattern)
    );
}

#[test]
fn hamming_sliding_window_reports_validation_errors() {
    assert!(homomorphic_sliding_window(&[], &[0]).is_err());
    assert!(homomorphic_sliding_window(&trivial_values(&[0]), &[]).is_err());
    assert!(homomorphic_sliding_window(&trivial_values(&[0]), &[0, 1]).is_err());

    let long_word = trivial_values(&vec![0; 257]);
    let long_pattern = vec![0; 256];
    assert!(homomorphic_sliding_window(&long_word, &long_pattern).is_err());
}

#[test]
fn encrypted_hamming_sliding_window_reports_validation_errors() {
    assert!(homomorphic_sliding_window_encrypted(&[], &trivial_values(&[0])).is_err());
    assert!(homomorphic_sliding_window_encrypted(&trivial_values(&[0]), &[]).is_err());
    assert!(
        homomorphic_sliding_window_encrypted(&trivial_values(&[0]), &trivial_values(&[0, 1]))
            .is_err()
    );
}

#[test]
fn hamming_database_comparison_runs_multiple_patterns() {
    let word = [0, 1, 2, 3, 0];
    let patterns = vec![encrypted_pattern(1, "AT"), encrypted_pattern(2, "CG")];

    let results = compare_against_encrypted_patterns(&trivial_values(&word), &patterns).unwrap();

    assert_eq!(
        clear_values(&results[0]),
        expected_hamming_windows(&word, &[0, 1])
    );
    assert_eq!(
        clear_values(&results[1]),
        expected_hamming_windows(&word, &[2, 3])
    );
}

#[test]
fn process_hamming_serializes_expected_window_distances() {
    let word = [0, 1, 2, 3, 0];
    let response = process_hamming(
        process_request(&word),
        risk_pattern(1, "AT"),
        session_keys(),
    )
    .unwrap();

    assert_eq!(response.windows, 4);
    assert_eq!(
        clear_serialized_values(&response.encrypted_distance_items),
        expected_hamming_windows(&word, &[0, 1])
    );
}

#[test]
fn compare_database_hamming_encrypts_patterns_and_returns_all_windows() {
    let word = [0, 1, 2, 3, 0];
    let patterns = vec![risk_pattern(1, "AT"), risk_pattern(2, "CG")];

    let response = compare_database(
        compare_database_request(&word),
        patterns.clone(),
        session_keys(),
    )
    .unwrap();

    assert_eq!(response.compared_sequences, 2);
    assert_eq!(response.patterns[0].sequence, patterns[0].sequence);
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[0]),
        expected_hamming_windows(&word, &[0, 1])
    );
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[1]),
        expected_hamming_windows(&word, &[2, 3])
    );
}

#[test]
fn compare_session_sequences_hamming_uses_shorter_sequence_as_pattern() {
    let session_id = "session-hamming";
    let word = [0, 1, 2, 3, 0];
    let stored = vec![stored_session_sequence(1, session_id, &[0, 1])];

    let response = compare_session_sequences_hamming(
        compare_session_sequences_request(session_id, &word),
        stored,
        session_keys(),
    )
    .unwrap();

    assert_eq!(response.compared_sequences, 1);
    assert_eq!(response.sequences[0].session_id, session_id);
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[0]),
        expected_hamming_windows(&word, &[0, 1])
    );
}

#[test]
fn encrypted_counting_values_are_public_trivial_counts() {
    install_test_server_key();

    let counts = encrypted_counting_values(3, &FheUint8::encrypt_trivial(0u8));

    assert_eq!(clear_values(&counts), vec![0, 1, 2, 3]);
}

#[test]
fn levenshtein_cell_handles_match_mismatch_insertion_and_deletion_costs() {
    let seq = trivial_values(&[0]);
    let zero = FheUint8::encrypt_trivial(0u8);
    let one = FheUint8::encrypt_trivial(1u8);
    let dp = vec![
        vec![zero.clone(), one.clone()],
        vec![one.clone(), zero.clone()],
    ];

    assert_eq!(clear_value(&levenshtein_cell(&seq, &[0], &dp, 1, 1)), 0);
    assert_eq!(clear_value(&levenshtein_cell(&seq, &[1], &dp, 1, 1)), 1);
}

#[test]
fn encrypted_levenshtein_cell_handles_match_and_mismatch() {
    let seq = trivial_values(&[0]);
    let zero = FheUint8::encrypt_trivial(0u8);
    let one = FheUint8::encrypt_trivial(1u8);
    let dp = vec![
        vec![zero.clone(), one.clone()],
        vec![one.clone(), zero.clone()],
    ];

    assert_eq!(
        clear_value(&levenshtein_cell_encrypted(
            &seq,
            &trivial_values(&[0]),
            &dp,
            1,
            1
        )),
        0
    );
    assert_eq!(
        clear_value(&levenshtein_cell_encrypted(
            &seq,
            &trivial_values(&[1]),
            &dp,
            1,
            1
        )),
        1
    );
}

#[test]
fn levenshtein_distance_matches_plain_model_for_small_patterns() {
    for (left, right) in [
        (&[0][..], &[0][..]),
        (&[0][..], &[1][..]),
        (&[0, 1][..], &[0][..]),
        (&[0, 1, 2][..], &[0, 2][..]),
    ] {
        let distance = homomorphic_levenshtein_distance(&trivial_values(left), right).unwrap();
        assert_eq!(clear_value(&distance), expected_levenshtein(left, right));
    }
}

#[test]
fn encrypted_levenshtein_distance_matches_plain_model_for_small_patterns() {
    for (left, right) in [
        (&[0][..], &[0][..]),
        (&[0][..], &[1][..]),
        (&[0, 1][..], &[0][..]),
        (&[0, 1, 2][..], &[0, 2][..]),
    ] {
        let distance = homomorphic_levenshtein_distance_encrypted(
            &trivial_values(left),
            &trivial_values(right),
        )
        .unwrap();
        assert_eq!(clear_value(&distance), expected_levenshtein(left, right));
    }
}

#[test]
fn levenshtein_distance_reports_validation_errors() {
    assert!(homomorphic_levenshtein_distance(&[], &[0]).is_err());
    assert!(homomorphic_levenshtein_distance(&trivial_values(&[0]), &[]).is_err());
    assert!(homomorphic_levenshtein_distance(&trivial_values(&vec![0; 256]), &[0]).is_err());
}

#[test]
fn encrypted_levenshtein_distance_reports_validation_errors() {
    assert!(homomorphic_levenshtein_distance_encrypted(&[], &trivial_values(&[0])).is_err());
    assert!(homomorphic_levenshtein_distance_encrypted(&trivial_values(&[0]), &[]).is_err());
    assert!(homomorphic_levenshtein_distance_encrypted(
        &trivial_values(&vec![0; 256]),
        &trivial_values(&[0])
    )
    .is_err());
}

#[test]
fn levenshtein_database_comparison_runs_multiple_patterns() {
    let word = [0, 1, 2];
    let patterns = vec![encrypted_pattern(1, "ATC"), encrypted_pattern(2, "AC")];

    let results =
        compare_against_encrypted_patterns_levenshtein(&trivial_values(&word), &patterns).unwrap();

    assert_eq!(
        clear_value(&results[0]),
        expected_levenshtein(&word, &[0, 1, 2])
    );
    assert_eq!(
        clear_value(&results[1]),
        expected_levenshtein(&word, &[0, 2])
    );
}

#[test]
fn process_levenshtein_serializes_single_distance() {
    let word = [0];
    let response =
        process_levenshtein(process_request(&word), risk_pattern(1, "A"), session_keys()).unwrap();

    assert_eq!(response.windows, 1);
    assert_eq!(
        clear_serialized_values(&response.encrypted_distance_items),
        vec![expected_levenshtein(&word, &[0])]
    );
}

#[test]
fn compare_database_levenshtein_encrypts_patterns_and_returns_distances() {
    let word = [0, 1, 2];
    let patterns = vec![risk_pattern(1, "ATC"), risk_pattern(2, "AC")];

    let response = compare_database_levenshtein(
        compare_database_request(&word),
        patterns.clone(),
        session_keys(),
    )
    .unwrap();

    assert_eq!(response.compared_sequences, 2);
    assert_eq!(response.patterns[1].sequence, patterns[1].sequence);
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[0]),
        vec![expected_levenshtein(&word, &[0, 1, 2])]
    );
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[1]),
        vec![expected_levenshtein(&word, &[0, 2])]
    );
}

#[test]
fn compare_session_sequences_levenshtein_returns_one_distance_per_sequence() {
    let session_id = "session-levenshtein";
    let word = [0, 1, 2];
    let stored = vec![
        stored_session_sequence(1, session_id, &[0, 1, 2]),
        stored_session_sequence(2, session_id, &[0, 2]),
    ];

    let response = compare_session_sequences_levenshtein(
        compare_session_sequences_request(session_id, &word),
        stored,
        session_keys(),
    )
    .unwrap();

    assert_eq!(response.compared_sequences, 2);
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[0]),
        vec![expected_levenshtein(&word, &[0, 1, 2])]
    );
    assert_eq!(
        clear_serialized_values(&response.encrypted_result_items[1]),
        vec![expected_levenshtein(&word, &[0, 2])]
    );
}
