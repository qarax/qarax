use chrono::Utc;
use serde::Serialize;
use std::sync::OnceLock;
use tokio::sync::broadcast;
use uuid::Uuid;

const CHANNEL_CAPACITY: usize = 128;

static EVENT_TX: OnceLock<broadcast::Sender<VmStatusEvent>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
pub struct VmStatusEvent {
    pub event: String,
    pub timestamp: String,
    pub vm_id: Uuid,
    pub vm_name: String,
    pub previous_status: String,
    pub new_status: String,
    pub host_id: Option<Uuid>,
    pub tags: Vec<String>,
}

/// Initialize the global event bus. Idempotent — safe to call multiple times (e.g., in tests).
pub fn init_event_bus() {
    let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
    let _ = EVENT_TX.set(tx);
}

/// Emit a VM status change event. Best-effort: silently drops if no subscribers.
pub fn emit(
    vm_id: Uuid,
    vm_name: &str,
    previous_status: &str,
    new_status: &str,
    host_id: Option<Uuid>,
    tags: &[String],
) {
    if let Some(tx) = EVENT_TX.get() {
        let event = VmStatusEvent {
            event: "vm.status_changed".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            vm_id,
            vm_name: vm_name.to_string(),
            previous_status: previous_status.to_string(),
            new_status: new_status.to_string(),
            host_id,
            tags: tags.to_vec(),
        };
        let _ = tx.send(event);
    }
}

/// Subscribe to the event stream. Returns a receiver that yields all future events.
pub fn subscribe() -> broadcast::Receiver<VmStatusEvent> {
    EVENT_TX
        .get()
        .expect("event bus not initialized — call init_event_bus() first")
        .subscribe()
}
