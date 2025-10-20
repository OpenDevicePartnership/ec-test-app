use color_eyre::{Result, eyre::eyre};
use std::sync::{atomic, mpsc};

#[cfg(not(feature = "mock"))]
unsafe extern "C" {
    fn InitializeNotification() -> i32;
    fn WaitForNotification(event: u32) -> u32;
    fn CleanupNotification();
}

#[cfg(feature = "mock")]
use mock::*;
#[cfg(feature = "mock")]
#[allow(non_snake_case)]
mod mock {
    pub(super) unsafe fn InitializeNotification() -> i32 {
        // Do nothing for mock
        0
    }

    pub(super) unsafe fn WaitForNotification(event: u32) -> u32 {
        // Just wait for a little bit then return the event that was passed in
        std::thread::sleep(std::time::Duration::from_millis(500));
        event
    }

    pub(super) unsafe fn CleanupNotification() {
        // Do nothing for mock
    }
}

const RX_BUF_SZ: usize = 128;

/// A notification event
#[derive(Debug, Copy, Clone)]
pub enum Event {
    Any,
    DbgFrameAvailable,
}

// Eventually would want to make this configurable to support multiple platforms
// But for now hardcode values
impl From<Event> for u32 {
    fn from(event: Event) -> Self {
        match event {
            Event::Any => 0,
            Event::DbgFrameAvailable => 20,
        }
    }
}

impl TryFrom<u32> for Event {
    type Error = color_eyre::Report;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Any),
            20 => Ok(Self::DbgFrameAvailable),
            _ => Err(eyre!("Unknown event received")),
        }
    }
}

pub struct EventRx<T> {
    rx: std::sync::mpsc::Receiver<T>,
    signal_with_guard: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl<T> EventRx<T> {
    /// Start the event receiver
    pub fn start(&mut self) {
        let (guard, signal) = &*self.signal_with_guard;
        *guard.lock().expect("Guard must not be poisoned") = true;
        signal.notify_one();
    }

    /// Stop the event receiver
    pub fn stop(&mut self) {
        let (guard, _signal) = &*self.signal_with_guard;
        *guard.lock().expect("Guard must not be poisoned") = false;
    }

    /// Returns the most recent data in the rx buffer if any
    pub fn receive(&self) -> Option<T> {
        match self.rx.try_recv() {
            Ok(data) => Some(data),
            Err(mpsc::TryRecvError::Empty) => None,

            // Choose to panic here for caller ergonomics
            // This case shouldn't happen in this app and is pretty much unrecoverable
            Err(mpsc::TryRecvError::Disconnected) => panic!("Polled dropped notification service"),
        }
    }
}

/// Singleton notification service
static INITIALIZED: atomic::AtomicBool = atomic::AtomicBool::new(false);
pub struct Notifications;

impl Notifications {
    /// Create and initialize a new notification service.
    ///
    /// Returns an error if notification service instance already exists.
    pub fn new() -> Result<Self> {
        if INITIALIZED
            .compare_exchange(false, true, atomic::Ordering::SeqCst, atomic::Ordering::SeqCst)
            .is_ok()
        {
            // SAFETY: Only a single instance will ever exist at once
            let res = unsafe { InitializeNotification() };
            if res == 0 {
                Ok(Self)
            } else {
                INITIALIZED.store(false, atomic::Ordering::SeqCst);
                Err(eyre!("Failed to initialize notification service"))
            }
        } else {
            Err(eyre!("Only one notification service must exist at a time"))
        }
    }

    /// Creates an event receiver `EventRx` which spawns a thread that waits for specified event.
    ///
    /// This receiver will then use the provided closure to perform some action and return data whenever event is received.
    ///
    /// This returned data is automatically stored in a buffer which caller can access via `EventRx::receive`.
    pub fn event_receiver<T: Send + 'static>(
        &self,
        event: Event,
        f: impl Fn(Event) -> T + Send + 'static,
    ) -> EventRx<T> {
        let (tx, rx) = mpsc::sync_channel::<T>(RX_BUF_SZ);
        let signal_with_guard = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let waiter = std::sync::Arc::clone(&signal_with_guard);

        std::thread::spawn(move || {
            let (guard, signal) = &*waiter;

            loop {
                // Check if we should still run, and if not, sleep until told to start again
                {
                    let mut running = guard.lock().expect("Guard must not be poisoned");
                    while !*running {
                        running = signal.wait(running).expect("Guard must not be poisoned");
                    }
                }

                // If we somehow receive a notification that we didn't intend, just discard it
                if let Ok(event) = Self::wait_event(event) {
                    let data = f(event);

                    // Receiver has dropped, so just end the thread silently
                    if tx.send(data).is_err() {
                        break;
                    }
                }
            }
        });

        EventRx { rx, signal_with_guard }
    }

    fn wait_event(event: Event) -> Result<Event> {
        // SAFETY: Driver can handle multiple threads calling simultaneously
        let recv = unsafe { WaitForNotification(event.into()) };
        Event::try_from(recv)
    }
}

impl Drop for Notifications {
    fn drop(&mut self) {
        // SAFETY: This is only called once automatically when singleton service is dropped
        unsafe { CleanupNotification() };
        INITIALIZED.store(false, atomic::Ordering::SeqCst);
    }
}
