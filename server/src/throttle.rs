use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::debug;

/// Represents a throttle entry for tracking failed attempts
#[derive(Debug, Clone)]
struct ThrottleEntry {
    attempts: u32,
    last_attempt: Instant,
    locked_until: Option<Instant>,
}

/// In-memory throttling service for registration attempts
#[derive(Debug)]
pub struct RegistrationThrottleService {
    attempts: Arc<Mutex<HashMap<String, ThrottleEntry>>>,
}

impl RegistrationThrottleService {
    pub fn new() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a registration attempt is allowed for the given identifier
    /// Returns Ok(()) if allowed, Err with remaining seconds if blocked
    pub fn check_attempt(
        &self,
        identifier: &str,
        max_attempts: u32,
        lockout_seconds: u32,
    ) -> Result<(), u32> {
        let mut attempts = self.attempts.lock().unwrap();
        let now = Instant::now();

        // Clean up old entries periodically
        self.cleanup_old_entries(&mut attempts, now, lockout_seconds);

        if let Some(entry) = attempts.get(identifier).cloned() {
            // Check if currently locked out
            if let Some(locked_until) = entry.locked_until {
                if now < locked_until {
                    let remaining = locked_until.duration_since(now).as_secs() as u32;
                    debug!(
                        "Registration attempt blocked for {}: {} seconds remaining",
                        identifier, remaining
                    );
                    return Err(remaining);
                }
            }

            // Check if we've exceeded max attempts
            if entry.attempts >= max_attempts {
                let lockout_duration = Duration::from_secs(lockout_seconds as u64);
                let locked_until = entry.last_attempt + lockout_duration;

                if now < locked_until {
                    let remaining = locked_until.duration_since(now).as_secs() as u32;
                    // Update the entry to mark it as locked
                    attempts.insert(
                        identifier.to_string(),
                        ThrottleEntry {
                            attempts: entry.attempts,
                            last_attempt: entry.last_attempt,
                            locked_until: Some(locked_until),
                        },
                    );
                    debug!(
                        "Registration attempts exceeded for {}: {} seconds lockout",
                        identifier, remaining
                    );
                    return Err(remaining);
                }
            }
        }

        debug!("Registration attempt allowed for {}", identifier);
        Ok(())
    }

    /// Record a failed registration attempt
    pub fn record_failed_attempt(&self, identifier: &str, max_attempts: u32, lockout_seconds: u32) {
        let mut attempts = self.attempts.lock().unwrap();
        let now = Instant::now();

        let entry = attempts
            .entry(identifier.to_string())
            .or_insert(ThrottleEntry {
                attempts: 0,
                last_attempt: now,
                locked_until: None,
            });

        entry.attempts += 1;
        entry.last_attempt = now;

        // If we've reached the max attempts, set the lockout
        if entry.attempts >= max_attempts {
            let lockout_duration = Duration::from_secs(lockout_seconds as u64);
            entry.locked_until = Some(now + lockout_duration);
            debug!(
                "Registration lockout triggered for {}: {} attempts, locked for {} seconds",
                identifier, entry.attempts, lockout_seconds
            );
        } else {
            debug!(
                "Registration failed attempt recorded for {}: {}/{} attempts",
                identifier, entry.attempts, max_attempts
            );
        }
    }

    /// Record a successful registration attempt (clears the throttle entry)
    pub fn record_successful_attempt(&self, identifier: &str) {
        let mut attempts = self.attempts.lock().unwrap();
        attempts.remove(identifier);
        debug!(
            "Registration throttle cleared for successful attempt: {}",
            identifier
        );
    }

    /// Get current attempt count for an identifier
    pub fn get_attempt_count(&self, identifier: &str) -> u32 {
        let attempts = self.attempts.lock().unwrap();
        attempts.get(identifier).map_or(0, |entry| entry.attempts)
    }

    /// Clean up old entries to prevent memory bloat
    fn cleanup_old_entries(
        &self,
        attempts: &mut HashMap<String, ThrottleEntry>,
        now: Instant,
        lockout_seconds: u32,
    ) {
        let cleanup_threshold = Duration::from_secs(lockout_seconds as u64 * 2); // Keep entries for 2x lockout period
        attempts.retain(|_, entry| {
            // Remove entries that are old and not locked
            if entry.locked_until.is_none() {
                now.duration_since(entry.last_attempt) < cleanup_threshold
            } else {
                // Keep locked entries until they expire
                entry
                    .locked_until
                    .map_or(false, |locked_until| now < locked_until)
            }
        });
    }

    /// Get remaining lockout time for an identifier, if any
    pub fn get_remaining_lockout(&self, identifier: &str) -> Option<u32> {
        let attempts = self.attempts.lock().unwrap();
        let now = Instant::now();

        if let Some(entry) = attempts.get(identifier) {
            if let Some(locked_until) = entry.locked_until {
                if now < locked_until {
                    return Some(locked_until.duration_since(now).as_secs() as u32);
                }
            }
        }
        None
    }
}

impl Default for RegistrationThrottleService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_throttle_basic_functionality() {
        let service = RegistrationThrottleService::new();
        let identifier = "test_user";
        let max_attempts = 3;
        let lockout_seconds = 60;

        // First few attempts should be allowed
        assert!(service
            .check_attempt(identifier, max_attempts, lockout_seconds)
            .is_ok());
        service.record_failed_attempt(identifier, max_attempts, lockout_seconds);
        assert_eq!(service.get_attempt_count(identifier), 1);

        assert!(service
            .check_attempt(identifier, max_attempts, lockout_seconds)
            .is_ok());
        service.record_failed_attempt(identifier, max_attempts, lockout_seconds);
        assert_eq!(service.get_attempt_count(identifier), 2);

        assert!(service
            .check_attempt(identifier, max_attempts, lockout_seconds)
            .is_ok());
        service.record_failed_attempt(identifier, max_attempts, lockout_seconds);
        assert_eq!(service.get_attempt_count(identifier), 3);

        // After max attempts, should be blocked
        let result = service.check_attempt(identifier, max_attempts, lockout_seconds);
        assert!(result.is_err());
        let remaining = result.unwrap_err();
        assert!(remaining > 0 && remaining <= lockout_seconds);
    }

    #[test]
    fn test_successful_attempt_clears_throttle() {
        let service = RegistrationThrottleService::new();
        let identifier = "test_user";
        let max_attempts = 3;
        let lockout_seconds = 60;

        // Record failed attempts
        service.record_failed_attempt(identifier, max_attempts, lockout_seconds);
        service.record_failed_attempt(identifier, max_attempts, lockout_seconds);
        assert_eq!(service.get_attempt_count(identifier), 2);

        // Successful attempt should clear the count
        service.record_successful_attempt(identifier);
        assert_eq!(service.get_attempt_count(identifier), 0);
    }
}
