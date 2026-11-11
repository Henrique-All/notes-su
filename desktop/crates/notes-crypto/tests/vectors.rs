use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use chrono::Utc;
use notes_core::{BackupData, Note};
use notes_crypto::*;

#[test]
fn vector_1_canonical_vault_canary_matches_libsodium_exact_bytes() {
    let password = "correct horse battery staple";
    let salt: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    let iv: [u8; 24] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
    ];
    let kdf = KdfParams::default();

    let key = derive_key(password, &salt, &kdf).expect("derive Argon2id key");
    assert_eq!(
        hex::encode(&*key),
        "d825a2b0e96c51c7e4bad361ea705b1cc25a84d7fa01ad832db4798c99102c69"
    );

    let env = encrypt_with_salt_and_iv(password, VAULT_CANARY_PLAINTEXT, salt, iv, kdf)
        .expect("encrypt deterministic vector 1");
    assert_eq!(env.alg, "xchacha20poly1305-argon2id-v1");
    assert_eq!(env.salt, "AAECAwQFBgcICQoLDA0ODw==");
    assert_eq!(env.iv, "EBESExQVFhcYGRobHB0eHyAhIiMkJSYn");
    assert_eq!(
        env.cipher,
        "BE13bnqsaTK2cm0EOzfKneXdL0SzZM/j8Dl5mmohs8oBKN4i5cti/g=="
    );
    assert_eq!(
        hex::encode(BASE64_STD.decode(&env.cipher).unwrap()),
        "044d776e7aac6932b6726d043b37ca9de5dd2f44b364cfe3f039799a6a21b3ca0128de22e5cb62fe"
    );
    assert_eq!(env.length, 24);

    let decrypted = decrypt_envelope(password, &env).expect("decrypt vector 1");
    assert_eq!(decrypted.as_str(), VAULT_CANARY_PLAINTEXT);
    verify_vault_canary(password, &env).expect("verify canary");
}

#[test]
fn vector_2_multibyte_utf8_portuguese_emoji_markdown_matches_libsodium() {
    let password = "SenhaSuperSecreta!2026#Çãõ";
    let salt: [u8; 16] = std::array::from_fn(|i| 0xa0u8 + i as u8);
    let iv: [u8; 24] = std::array::from_fn(|i| 0xb0u8 + i as u8);
    let plaintext = "# Notas Pessoais 🔒\n\n- [x] Configurar cofre criptografado (Argon2id + XChaCha20-Poly1305)\n- [ ] Testar sincronização Web ↔ GPUI!\n";

    let kdf = KdfParams::default();
    let key = derive_key(password, &salt, &kdf).expect("derive vector 2 key");
    assert_eq!(
        hex::encode(&*key),
        "2ff2a99f9415e21a0be35a367d15359a8b2821d932bef510970939a50d9f1641"
    );

    let env = encrypt_with_salt_and_iv(password, plaintext, salt, iv, kdf)
        .expect("encrypt vector 2");
    assert_eq!(env.salt, "oKGio6SlpqeoqaqrrK2urw==");
    assert_eq!(env.iv, "sLGys7S1tre4ubq7vL2+v8DBwsPExcbH");
    assert_eq!(
        env.cipher,
        "C+eB4qAyDqwYTwjI5CR6VN/0J472yGp4Gwc5VGa/5eHvoMrF7lbYLogoO81QCvMAG4PDc+Xg2QRhPbInjgOt4kYiw6t5weNBqzmdsnnZ9WZVugIVH+XXSwUY4+Bo+mjDBAKw4iCGRKbV5R2hNnoItc4WNyeKkmB008zCT01sCDyvwr/XiQT5tJTPFo5AfEe7JRsi9m+0pw=="
    );
    assert_eq!(env.length, 135);

    let decrypted = decrypt_envelope(password, &env).expect("decrypt vector 2");
    assert_eq!(decrypted.as_str(), plaintext);
}

#[test]
fn vector_3_empty_plaintext_tag_only_matches_libsodium() {
    let password = "correct horse battery staple";
    let salt: [u8; 16] = std::array::from_fn(|i| i as u8);
    let iv = [0xffu8; 24];
    let env = encrypt_with_salt_and_iv(password, "", salt, iv, KdfParams::default())
        .expect("encrypt empty plaintext");

    assert_eq!(env.iv, "////////////////////////////////");
    assert_eq!(env.cipher, "96co6DvhbAYRdqZlb/TJ2Q==");
    assert_eq!(
        hex::encode(BASE64_STD.decode(&env.cipher).unwrap()),
        "f7a728e83be16c061176a6656ff4c9d9"
    );
    assert_eq!(env.length, 0);

    let decrypted = decrypt_envelope(password, &env).expect("decrypt empty plaintext");
    assert_eq!(decrypted.as_str(), "");
}

#[test]
fn vector_4_canonical_nnbackup_sha256_digest_and_seal_unseal() {
    let raw_json: serde_json::Value = serde_json::from_str(
        r#"{"tags":[],"notes":[{"title":"Hello","id":"00000000-0000-4000-8000-000000000001"}],"notebooks":[],"colors":[]}"#,
    )
    .unwrap();
    let digest = compute_canonical_sha256(&raw_json).expect("canonical sha256");
    assert_eq!(
        digest,
        "57a0973d7d4cbfc62a2105e0ad417044c4339b97b229189e954489e5e78d915a"
    );

    // Encrypted & plain .nnbackup seal/unseal + tamper/version checks
    let now = Utc::now();
    let mut data = BackupData::default();
    data.changes.notes.push(Note {
        id: "00000000-0000-4000-8000-000000000001".to_owned(),
        user_id: None,
        title: "Backup Note".to_owned(),
        headline: "Body".to_owned(),
        is_generated_title: false,
        pinned: false,
        favorite: false,
        archived: false,
        readonly: false,
        local_only: false,
        conflicted: false,
        locked: false,
        color_id: None,
        expiry_date: None,
        expiry_modified_at: None,
        trashed_at: None,
        deleted_by: None,
        deleted: false,
        date_created: now,
        date_edited: now,
        date_modified: now,
        revision: 1,
    });

    let enc_bundle = seal_backup(&data, Some("backup-secret"), now).expect("seal encrypted");
    assert!(enc_bundle.encrypted);
    assert!(enc_bundle.data.is_none());
    assert!(enc_bundle.cipher.is_some());

    assert_eq!(
        unseal_backup(&enc_bundle, None),
        Err(CryptoError::PasswordRequired)
    );
    assert_eq!(
        unseal_backup(&enc_bundle, Some("wrong-pw")),
        Err(CryptoError::WrongPassword)
    );
    let restored = unseal_backup(&enc_bundle, Some("backup-secret")).expect("unseal encrypted");
    assert_eq!(restored.changes.notes.len(), 1);
    assert_eq!(restored.changes.notes[0].title, "Backup Note");

    let mut bad_version = enc_bundle.clone();
    bad_version.version = 2;
    assert_eq!(
        unseal_backup(&bad_version, Some("backup-secret")),
        Err(CryptoError::UnsupportedBackupVersion(2))
    );

    let mut bad_sha = seal_backup(&data, None, now).expect("seal plain");
    bad_sha.sha256 = "0".repeat(64);
    assert_eq!(unseal_backup(&bad_sha, None), Err(CryptoError::ChecksumMismatch));
}

#[test]
fn tamper_dos_bounds_atomic_rotation_monograph_and_vault_session() {
    let canary = create_vault_canary("old-vault-pw").expect("create canary");
    assert_eq!(
        verify_vault_canary("wrong-pw", &canary),
        Err(CryptoError::WrongPassword)
    );

    // Malformed salt/iv lengths never panic
    let mut bad_iv = canary.clone();
    bad_iv.iv = BASE64_STD.encode([0u8; 12]);
    assert_eq!(
        decrypt_envelope("old-vault-pw", &bad_iv),
        Err(CryptoError::InvalidNonceLength(12))
    );

    let mut bad_salt = canary.clone();
    bad_salt.salt = BASE64_STD.encode([0u8; 8]);
    assert_eq!(
        decrypt_envelope("old-vault-pw", &bad_salt),
        Err(CryptoError::InvalidSaltLength(8))
    );

    // Tampered Poly1305 tag is rejected
    let mut tampered = canary.clone();
    let mut raw_cipher = BASE64_STD.decode(&tampered.cipher).unwrap();
    *raw_cipher.last_mut().unwrap() ^= 0x01;
    tampered.cipher = BASE64_STD.encode(raw_cipher);
    assert_eq!(
        decrypt_envelope("old-vault-pw", &tampered),
        Err(CryptoError::DecryptionFailed)
    );

    // DoS KDF bounds check rejects 2 GiB memKiB
    let mut dos_env = canary.clone();
    dos_env.kdf.mem_kib = 2_097_152;
    assert!(matches!(
        decrypt_envelope("old-vault-pw", &dos_env),
        Err(CryptoError::InvalidKdfParams(_))
    ));

    // Lock two notes and test atomic 2-phase rotation
    let n1 = lock_note_content("old-vault-pw", &canary, "Secret Note 1").unwrap();
    let n2 = lock_note_content("old-vault-pw", &canary, "Secret Note 2").unwrap();
    assert_eq!(n1.headline, "");

    // Corrupted note #2 aborts entire rotation before generating any partial state
    let corrupted_batch = vec![
        ("n1".to_owned(), n1.data_json.clone()),
        ("n2".to_owned(), "{\"corrupted\":true}".to_owned()),
    ];
    assert_eq!(
        rotate_vault_password("old-vault-pw", "new-vault-pw", &canary, &corrupted_batch),
        Err(CryptoError::NoteDecryptionFailed {
            note_id: "n2".to_owned()
        })
    );

    // Valid batch rotates cleanly with fresh salts/IVs
    let valid_batch = vec![
        ("n1".to_owned(), n1.data_json.clone()),
        ("n2".to_owned(), n2.data_json.clone()),
    ];
    let (new_canary, rotated) =
        rotate_vault_password("old-vault-pw", "new-vault-pw", &canary, &valid_batch)
            .expect("atomic rotation");
    verify_vault_canary("new-vault-pw", &new_canary).unwrap();
    assert_eq!(
        unlock_note_content("new-vault-pw", &rotated[0].1)
            .unwrap()
            .as_str(),
        "Secret Note 1"
    );
    assert_eq!(
        unlock_note_content("new-vault-pw", &rotated[1].1)
            .unwrap()
            .as_str(),
        "Secret Note 2"
    );

    // Monograph encrypt/decrypt
    let mono_env = encrypt_monograph_content("reader-pw", "# Public Encrypted Note").unwrap();
    assert_eq!(
        decrypt_monograph_content("wrong", &mono_env),
        Err(CryptoError::WrongPassword)
    );
    assert_eq!(
        decrypt_monograph_content("reader-pw", &mono_env).unwrap(),
        "# Public Encrypted Note"
    );

    // VaultSession inactivity auto-lock & touch_on_save reset
    let mut session = VaultSession::new(60);
    session.unlock("new-vault-pw", &new_canary).unwrap();
    assert!(session.is_unlocked());
    assert!(!session.check_auto_lock_at(Instant::now() + Duration::from_secs(30)));
    assert!(session.check_auto_lock_at(Instant::now() + Duration::from_secs(61)));
    assert!(!session.is_unlocked());
}
