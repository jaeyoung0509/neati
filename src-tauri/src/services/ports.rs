//! Port vocabulary shared by the interface-agnostic use cases.
//!
//! A *driving* port is what an interface adapter (the CLI today, the window
//! later) may ask of the application. A *driven* port is what a use case needs
//! from the outside (a service, the OS) and is bound to a concrete adapter only
//! in `crate::composition`. Neither side names Tauri, a terminal, or stdout.

use std::future::Future;
use std::pin::Pin;

/// The future a port method returns: owned, `Send`, and object-safe, so ports
/// can be held as `Arc<dyn Port>` like the platform ports are.
pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a use case is doing right now, for an adapter to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    /// A location is being read.
    Scanning { location: String },
    /// A project's build output is being measured.
    Measuring { project: String, kind: String },
    /// One unit of a removal run has started.
    Removing {
        name: String,
        index: usize,
        total: usize,
    },
    /// An external source (an AI provider) is being asked.
    Checking { source: String },
}

/// Where a use case reports [`Activity`]. Reporting never fails and never
/// blocks the work; an adapter that shows nothing ignores it.
pub trait ActivityPort: Send + Sync {
    fn report(&self, activity: Activity);
}

/// An activity port that shows nothing.
pub struct Silent;

impl ActivityPort for Silent {
    fn report(&self, _activity: Activity) {}
}
