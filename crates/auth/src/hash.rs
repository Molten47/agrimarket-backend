use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2, Algorithm, Version, Params,
};
use crate::error::AuthError;

/// Hash a plaintext password using Argon2id.
/// 
/// - Salt: randomly generated per password via OsRng (cryptographically secure)
/// - Algorithm: Argon2id (hybrid of Argon2i + Argon2d — OWASP first choice)
/// - Memory: 19MB  — defeats GPU/ASIC parallel attacks
/// - Iterations: 2 — time cost
/// - Parallelism: 1 — single thread (sufficient at this memory cost)
/// 
/// The salt is embedded in the returned hash string — you store only the hash.
/// Format: $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    // Generate a unique random salt for this password
    let salt = SaltString::generate(&mut OsRng);

    // Configure Argon2id with OWASP recommended parameters
    let argon2 = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(
            19_456, // memory cost: 19MB in KiB
            2,      // time cost: iterations
            1,      // parallelism
            None,   // output length: default (32 bytes)
        )
        .map_err(|_| AuthError::HashFailed)?,
    );

    // Hash the password — salt is embedded in the output string
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| AuthError::HashFailed)?
        .to_string();

    Ok(hash)
}

/// Verify a plaintext password against a stored Argon2id hash.
/// 
/// Extracts the salt and parameters from the hash string automatically —
/// no need to store or pass the salt separately.
/// Returns Ok(()) if the password matches, Err(AuthError::InvalidCredentials) if not.
/// The error is always InvalidCredentials — never "wrong hash format" — so the
/// caller cannot distinguish a bad password from a malformed hash (timing-safe).
pub fn verify_password(password: &str, hash: &str) -> Result<(), AuthError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|_| AuthError::InvalidCredentials)?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AuthError::InvalidCredentials)
}