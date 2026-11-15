//! App lock: PIN (always available) plus optional OS biometric unlock.
//! The locked flag lives in Rust so reloading the webview cannot skip the lock screen.

use std::time::{Duration, Instant};

const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(30);

pub struct LockState {
    pub locked: bool,
    failed: u32,
    retry_after: Option<Instant>,
}

impl LockState {
    pub fn new(locked: bool) -> Self {
        Self {
            locked,
            failed: 0,
            retry_after: None,
        }
    }

    /// Checks `pin` against `stored` hash, throttling repeated failures.
    pub fn try_unlock(&mut self, stored: &str, pin: &str) -> Result<(), String> {
        if let Some(until) = self.retry_after {
            let now = Instant::now();
            if now < until {
                return Err(format!(
                    "Too many wrong PINs. Try again in {} s.",
                    (until - now).as_secs() + 1
                ));
            }
            self.retry_after = None;
        }
        if verify_pin(stored, pin) {
            self.locked = false;
            self.failed = 0;
            return Ok(());
        }
        self.failed += 1;
        if self.failed >= MAX_FAILED_ATTEMPTS {
            self.failed = 0;
            self.retry_after = Some(Instant::now() + LOCKOUT);
        }
        Err("Wrong PIN".to_string())
    }
}

pub fn validate_pin(pin: &str) -> Result<(), String> {
    if (4..=12).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err("PIN must be 4 to 12 digits".to_string())
    }
}

// ponytail: salted BLAKE3, not a slow KDF. The hash sits in the OS keychain and the lock
// gates the UI only (save files on disk are not encrypted); switch to argon2 if that changes.
pub fn hash_pin(pin: &str) -> String {
    let salt = uuid::Uuid::new_v4().simple().to_string();
    format!("{}:{}", salt, pin_digest(&salt, pin).to_hex())
}

pub fn verify_pin(stored: &str, pin: &str) -> bool {
    stored.split_once(':').is_some_and(|(salt, hash)| {
        // blake3::Hash equality is constant-time.
        blake3::Hash::from_hex(hash).ok() == Some(pin_digest(salt, pin))
    })
}

fn pin_digest(salt: &str, pin: &str) -> blake3::Hash {
    blake3::hash(format!("{salt}{pin}").as_bytes())
}

/// Whether OS biometric / Windows Hello verification can run on this machine.
#[cfg(windows)]
pub fn biometric_available() -> bool {
    use windows::Security::Credentials::UI::{
        UserConsentVerifier, UserConsentVerifierAvailability,
    };
    UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|op| op.join())
        .map(|a| a == UserConsentVerifierAvailability::Available)
        .unwrap_or(false)
}

/// Shows the Windows Hello prompt (fingerprint, face or device PIN). Blocks until answered.
#[cfg(windows)]
pub fn biometric_verify(message: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    let result = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(message))
        .and_then(|op| op.join())
        .map_err(|e| e.to_string())?;
    if result == UserConsentVerificationResult::Verified {
        Ok(())
    } else {
        Err("Verification was cancelled or failed".to_string())
    }
}

// ponytail: biometric is Windows-only for now; macOS (LocalAuthentication) and Linux (polkit)
// fall back to PIN. Add them here when someone needs them.
#[cfg(not(windows))]
pub fn biometric_available() -> bool {
    false
}

#[cfg(not(windows))]
pub fn biometric_verify(_message: &str) -> Result<(), String> {
    Err("Biometric unlock is not supported on this OS yet".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hash_and_throttle() {
        assert!(validate_pin("1234").is_ok());
        assert!(validate_pin("12a4").is_err());
        assert!(validate_pin("123").is_err());

        let stored = hash_pin("2468");
        assert_ne!(stored, hash_pin("2468"), "salt must differ per hash");
        assert!(verify_pin(&stored, "2468"));
        assert!(!verify_pin(&stored, "2469"));
        assert!(!verify_pin("garbage", "2468"));

        let mut lock = LockState::new(true);
        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert_eq!(lock.try_unlock(&stored, "0000").unwrap_err(), "Wrong PIN");
        }
        // Locked out: even the right PIN is refused until the timeout passes.
        assert!(lock
            .try_unlock(&stored, "2468")
            .unwrap_err()
            .starts_with("Too many"));
        assert!(lock.locked);
        lock.retry_after = Some(Instant::now());
        assert!(lock.try_unlock(&stored, "2468").is_ok());
        assert!(!lock.locked);
    }
}
