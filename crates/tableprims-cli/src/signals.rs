// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stopping a run on SIGINT or SIGTERM.
//!
//! The first SIGINT or SIGTERM sets a flag. The conversion checks it before
//! each row of both reads and once more before the workbook is published;
//! the run then stops and publishes a result with status `cancelled`. On
//! Unix the process then ends by that signal, as a program that cleans up
//! after a signal should, so a shell reports 130 or 143 and a shell loop
//! stops; elsewhere it exits 130 or 143. A second SIGINT or SIGTERM ends the
//! process at once by that signal's default action, publishing no result if
//! it has not yet.
//!
//! The handlers only store to atomic flags, or take the default action, as
//! `signal-hook` arranges; nothing else runs inside them.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use crate::xlsx_write::{EXIT_SIGNAL_INT, EXIT_SIGNAL_TERM};

/// The exit code of a run stopped by `signal`.
fn exit_code_of(signal: i32) -> u8 {
    if signal == SIGTERM {
        EXIT_SIGNAL_TERM
    } else {
        EXIT_SIGNAL_INT
    }
}

/// Flags set by SIGINT and SIGTERM.
pub(crate) struct Signals {
    /// Set by either signal; the conversion checks it.
    stop: Arc<AtomicBool>,
    terminate: Arc<AtomicBool>,
}

impl Signals {
    /// Installs the handlers for this process.
    pub(crate) fn install() -> io::Result<Self> {
        let signals = Self {
            stop: Arc::new(AtomicBool::new(false)),
            terminate: Arc::new(AtomicBool::new(false)),
        };
        // Until both actions of a signal are registered, a signal arriving
        // here can be lost; the window is a few instructions at start-up.
        for signal in [SIGINT, SIGTERM] {
            // Actions run in the order they are registered: a signal that
            // finds the flag already set takes its default action and ends
            // the process; otherwise it records which signal came and sets
            // the flag.
            flag::register_conditional_default(signal, Arc::clone(&signals.stop))?;
            if signal == SIGTERM {
                flag::register(signal, Arc::clone(&signals.terminate))?;
            }
            flag::register(signal, Arc::clone(&signals.stop))?;
        }
        Ok(signals)
    }

    /// The flag the conversion checks.
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.stop
    }

    /// Whether a signal has asked the run to stop.
    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The exit code of a run stopped by a signal: 143 when SIGTERM came,
    /// 130 otherwise.
    pub(crate) fn exit_code(&self) -> u8 {
        exit_code_of(self.signal())
    }

    fn signal(&self) -> i32 {
        if self.terminate.load(Ordering::SeqCst) {
            SIGTERM
        } else {
            SIGINT
        }
    }
}

/// Ends a run that a signal stopped, after its result is published. On Unix
/// the process ends by the signal whose exit code is `code`, so that a
/// parent shell sees death by signal; this returns only if that fails.
/// Elsewhere it returns at once and the caller exits with `code`.
pub(crate) fn end_by_signal(code: u8) {
    #[cfg(unix)]
    for signal in [SIGINT, SIGTERM] {
        if exit_code_of(signal) == code {
            let _ = signal_hook::low_level::emulate_default_handler(signal);
        }
    }
    #[cfg(not(unix))]
    let _ = code;
}
