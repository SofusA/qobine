use tokio::sync::broadcast::{self, Receiver, Sender};

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Notification {
    Error(String),
    Warning(String),
    Success(String),
    Info(String),
}

#[derive(Debug)]
pub struct NotificationBroadcast {
    tx: Sender<Notification>,
    rx: Receiver<Notification>,
}

impl NotificationBroadcast {
    pub fn new() -> Self {
        let (tx, rx) = broadcast::channel(20);
        Self { tx, rx }
    }

    pub fn send(&self, notification: Notification) {
        self.tx.send(notification).expect("infallible");
    }

    pub fn send_error(&self, message: String) {
        tracing::error!(message);
        self.tx
            .send(Notification::Error(message))
            .expect("infallible");
    }

    pub fn subscribe(&self) -> Receiver<Notification> {
        self.rx.resubscribe()
    }
}

impl Default for NotificationBroadcast {
    fn default() -> Self {
        Self::new()
    }
}
