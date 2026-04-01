//! Signal handling for the daemon
//!
//! This module provides async signal handling using signal-hook and
//! signal-hook-tokio.

use futures::stream::StreamExt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Signal types we handle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Graceful shutdown request (SIGTERM, SIGINT)
    Shutdown,
    /// Configuration reload request (SIGHUP)
    Reload,
}

/// Signal handler for the daemon
#[derive(Debug)]
pub struct SignalHandler {
    /// Receiver for shutdown signals
    shutdown_rx: broadcast::Receiver<Signal>,
    /// Receiver for reload signals
    reload_rx: broadcast::Receiver<Signal>,
    /// Sender for signals (kept for reference counting)
    #[allow(dead_code)]
    tx: broadcast::Sender<Signal>,
}

impl SignalHandler {
    /// Create a new signal handler
    ///
    /// # Errors
    ///
    /// Returns an error if signal registration fails
    pub fn new() -> crate::Result<Self> {
        let (tx, _) = broadcast::channel(16);
        let shutdown_rx = tx.subscribe();
        let reload_rx = tx.subscribe();

        let handler = Self {
            shutdown_rx,
            reload_rx,
            tx,
        };

        // Register signal handlers
        handler.register_signals()?;

        info!("Signal handlers registered: SIGTERM, SIGINT, SIGHUP");
        Ok(handler)
    }

    /// Register signal handlers
    fn register_signals(&self) -> crate::Result<()> {
        let tx = self.tx.clone();

        // Handle SIGTERM and SIGINT for graceful shutdown
        let tx_shutdown = tx.clone();
        let mut signals = signal_hook_tokio::Signals::new([
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
        ])
        .map_err(|e| crate::NicAutoSwitchError::InvalidInput(e.to_string()))?;

        tokio::spawn(async move {
            while let Some(signal) = signals.next().await {
                match signal {
                    signal_hook::consts::SIGTERM => {
                        info!("Received SIGTERM");
                        let _ = tx_shutdown.send(Signal::Shutdown);
                    }
                    signal_hook::consts::SIGINT => {
                        info!("Received SIGINT");
                        let _ = tx_shutdown.send(Signal::Shutdown);
                    }
                    _ => {}
                }
            }
        });

        // Handle SIGHUP for configuration reload
        let tx_reload = tx;
        let mut sighup = signal_hook_tokio::Signals::new([signal_hook::consts::SIGHUP])
            .map_err(|e| crate::NicAutoSwitchError::InvalidInput(e.to_string()))?;

        tokio::spawn(async move {
            while sighup.next().await.is_some() {
                info!("Received SIGHUP");
                let _ = tx_reload.send(Signal::Reload);
            }
        });

        Ok(())
    }

    /// Wait for the next shutdown signal
    pub async fn wait_shutdown(&mut self) -> crate::Result<Signal> {
        loop {
            match self.shutdown_rx.recv().await {
                Ok(signal) if signal == Signal::Shutdown => {
                    return Ok(signal);
                }
                Ok(_) => continue, // Ignore other signals
                Err(broadcast::error::RecvError::Closed) => {
                    warn!("Signal channel closed");
                    return Err(crate::NicAutoSwitchError::InvalidInput(
                        "Signal channel closed".to_string(),
                    ));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    warn!("Signal receiver lagged, continuing");
                    continue;
                }
            }
        }
    }

    /// Wait for the next reload signal
    pub async fn wait_reload(&mut self) -> crate::Result<Signal> {
        loop {
            match self.reload_rx.recv().await {
                Ok(signal) if signal == Signal::Reload => {
                    return Ok(signal);
                }
                Ok(_) => continue, // Ignore other signals
                Err(broadcast::error::RecvError::Closed) => {
                    warn!("Signal channel closed");
                    return Err(crate::NicAutoSwitchError::InvalidInput(
                        "Signal channel closed".to_string(),
                    ));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    warn!("Signal receiver lagged, continuing");
                    continue;
                }
            }
        }
    }

    /// Get a receiver for all signals
    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }
}

/// Future that resolves when shutdown is requested
pub struct ShutdownFuture {
    receiver: broadcast::Receiver<Signal>,
}

impl ShutdownFuture {
    /// Create a new shutdown future
    pub fn new(handler: &SignalHandler) -> Self {
        Self {
            receiver: handler.subscribe(),
        }
    }
}

impl Future for ShutdownFuture {
    type Output = crate::Result<Signal>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.receiver.try_recv() {
            Ok(signal) => Poll::Ready(Ok(signal)),
            Err(broadcast::error::TryRecvError::Empty) => {
                // Schedule a wake-up and return Pending
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(broadcast::error::TryRecvError::Closed) => Poll::Ready(Err(
                crate::NicAutoSwitchError::InvalidInput("Signal channel closed".to_string()),
            )),
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                // Lagged is recoverable, just continue polling
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

/// Create a signal handler (convenience function)
pub fn create_signal_handler() -> crate::Result<SignalHandler> {
    SignalHandler::new()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_type_equality() {
        assert_eq!(Signal::Shutdown, Signal::Shutdown);
        assert_eq!(Signal::Reload, Signal::Reload);
        assert_ne!(Signal::Shutdown, Signal::Reload);
    }

    #[test]
    fn test_signal_debug_format() {
        let shutdown = Signal::Shutdown;
        let reload = Signal::Reload;

        assert!(format!("{:?}", shutdown).contains("Shutdown"));
        assert!(format!("{:?}", reload).contains("Reload"));
    }

    // Note: Testing actual signal handling is complex and typically
    // done in integration tests. Here we only test the basic structure.

    #[tokio::test]
    async fn test_signal_handler_creation() {
        // This test verifies that the signal handler can be created
        // without errors. Actual signal delivery is tested elsewhere.
        let result = SignalHandler::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_signal_copy_clone() {
        let signal = Signal::Shutdown;
        let signal_copy = signal;
        let signal_clone = signal;

        assert_eq!(signal, signal_copy);
        assert_eq!(signal, signal_clone);
    }

    // -------------------------------------------------------------------------
    // Broadcast channel and subscribe tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_returns_active_receiver() {
        let handler = SignalHandler::new().unwrap();
        let mut rx = handler.subscribe();
        // Receiver should be usable
        assert!(rx.try_recv().is_err()); // Empty initially
    }

    #[tokio::test]
    async fn test_wait_shutdown_receives_shutdown_signal() {
        let mut handler = SignalHandler::new().unwrap();
        // Send shutdown via broadcast
        handler.tx.send(Signal::Shutdown).unwrap();
        let result = handler.wait_shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_shutdown_ignores_reload_signal() {
        let mut handler = SignalHandler::new().unwrap();
        // Send reload first, then shutdown
        handler.tx.send(Signal::Reload).unwrap();
        handler.tx.send(Signal::Shutdown).unwrap();
        let result = handler.wait_shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_reload_receives_reload_signal() {
        let mut handler = SignalHandler::new().unwrap();
        handler.tx.send(Signal::Reload).unwrap();
        let result = handler.wait_reload().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_reload_ignores_shutdown_signal() {
        let mut handler = SignalHandler::new().unwrap();
        handler.tx.send(Signal::Shutdown).unwrap();
        handler.tx.send(Signal::Reload).unwrap();
        let result = handler.wait_reload().await;
        assert!(result.is_ok());
    }

    // -------------------------------------------------------------------------
    // ShutdownFuture tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_shutdown_future_poll_returns_pending_on_empty() {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        let handler = SignalHandler::new().unwrap();
        let mut future = ShutdownFuture::new(&handler);

        fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        let poll = std::pin::Pin::new(&mut future).poll(&mut cx);
        assert!(matches!(poll, Poll::Pending));
    }

    #[tokio::test]
    async fn test_shutdown_future_poll_returns_ready_on_signal() {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        let handler = SignalHandler::new().unwrap();
        let mut future = ShutdownFuture::new(&handler);
        handler.tx.send(Signal::Shutdown).unwrap();

        fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        let poll = std::pin::Pin::new(&mut future).poll(&mut cx);
        assert!(matches!(poll, Poll::Ready(Ok(Signal::Shutdown))));
    }

    #[tokio::test]
    async fn test_create_signal_handler_returns_handler() {
        let result = create_signal_handler();
        assert!(result.is_ok());
    }

    // -------------------------------------------------------------------------
    // Lagged receiver tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_wait_shutdown_handles_lagged_continues() {
        let mut handler = SignalHandler::new().unwrap();
        // Overflow the broadcast buffer (capacity 16)
        for _ in 0..20 {
            handler.tx.send(Signal::Reload).unwrap();
        }
        // Send shutdown signal at the end
        handler.tx.send(Signal::Shutdown).unwrap();
        let result = handler.wait_shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_reload_handles_lagged_continues() {
        let mut handler = SignalHandler::new().unwrap();
        for _ in 0..20 {
            handler.tx.send(Signal::Shutdown).unwrap();
        }
        handler.tx.send(Signal::Reload).unwrap();
        let result = handler.wait_reload().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_future_poll_handles_lagged() {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        let handler = SignalHandler::new().unwrap();
        let mut future = ShutdownFuture::new(&handler);

        // Overflow the buffer
        for _ in 0..20 {
            handler.tx.send(Signal::Shutdown).unwrap();
        }

        fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        // First poll might hit Lagged → Pending, or Ready with a signal
        let poll = std::pin::Pin::new(&mut future).poll(&mut cx);
        // Either Pending (lagged) or Ready is acceptable
        assert!(matches!(poll, Poll::Pending | Poll::Ready(Ok(_))));
    }
}
