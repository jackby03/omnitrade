//! Emergency kill switch for stopping trading operations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Emergency trading halt mechanism.
///
/// When activated, signals all trading systems to immediately close open positions
/// and stop accepting new orders. Broadcasts events to all subscribers.
pub struct KillSwitch {
    /// Internal tripped flag
    tripped: Arc<AtomicBool>,
    /// Broadcast channel for subscribers
    tx: broadcast::Sender<KillSwitchEvent>,
}

/// Events broadcast when kill switch is activated.
#[derive(Debug, Clone, Copy)]
pub enum KillSwitchEvent {
    /// Kill switch has been activated, trading must stop
    Activated,
}

impl KillSwitch {
    /// Creates a new KillSwitch with a broadcast channel.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tripped: Arc::new(AtomicBool::new(false)),
            tx,
        }
    }

    /// Activates the kill switch, signaling all subscribers.
    ///
    /// This operation is idempotent — calling it multiple times has the same effect
    /// as calling it once. It immediately broadcasts to all listeners.
    pub fn trip(&self) {
        let was_already_tripped = self.tripped.swap(true, Ordering::SeqCst);

        if !was_already_tripped {
            // Only broadcast once; ignore if no subscribers
            let _ = self.tx.send(KillSwitchEvent::Activated);
        }
    }

    /// Returns whether the kill switch has been activated.
    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    /// Creates a new broadcast receiver for kill switch events.
    pub fn subscribe(&self) -> broadcast::Receiver<KillSwitchEvent> {
        self.tx.subscribe()
    }

    /// Resets the kill switch (for testing purposes).
    #[cfg(test)]
    pub fn reset(&self) {
        self.tripped.store(false, Ordering::SeqCst);
    }
}

impl Default for KillSwitch {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for KillSwitch {
    /// Clones the KillSwitch, sharing the same atomic flag and broadcast channel.
    fn clone(&self) -> Self {
        Self {
            tripped: Arc::clone(&self.tripped),
            tx: self.tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_switch_creation() {
        let ks = KillSwitch::new();
        assert!(!ks.is_tripped());
    }

    #[test]
    fn test_kill_switch_trip() {
        let ks = KillSwitch::new();
        assert!(!ks.is_tripped());

        ks.trip();
        assert!(ks.is_tripped());
    }

    #[test]
    fn test_kill_switch_idempotent_trip() {
        let ks = KillSwitch::new();
        ks.trip();
        ks.trip();
        ks.trip();
        assert!(ks.is_tripped());
    }

    #[tokio::test]
    async fn test_kill_switch_broadcast() {
        let ks = KillSwitch::new();
        let mut rx = ks.subscribe();

        ks.trip();

        // Should receive the activated event
        match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
            Ok(Ok(KillSwitchEvent::Activated)) => {
                // Expected
            }
            _ => panic!("Expected to receive Activated event"),
        }
    }

    #[test]
    fn test_kill_switch_clone_shared_state() {
        let ks1 = KillSwitch::new();
        let ks2 = ks1.clone();

        assert!(!ks1.is_tripped());
        assert!(!ks2.is_tripped());

        ks1.trip();

        assert!(ks1.is_tripped());
        assert!(ks2.is_tripped());
    }

    #[test]
    fn test_kill_switch_multiple_subscribers() {
        let ks = KillSwitch::new();
        let _rx1 = ks.subscribe();
        let _rx2 = ks.subscribe();

        // Trip should broadcast to both subscribers
        ks.trip();
        assert!(ks.is_tripped());
    }

    #[test]
    fn test_default_create() {
        let ks = KillSwitch::default();
        assert!(!ks.is_tripped());
    }
}
