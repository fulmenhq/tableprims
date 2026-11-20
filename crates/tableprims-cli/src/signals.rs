// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stopping a run on SIGINT or SIGTERM.
//!
//! The first SIGINT or SIGTERM sets a flag. The conversion checks it before
//! each row of both reads and once more before the workbook is published;
//! the run then stops, publishes a result with status `cancelled` and exits
//! 130 for SIGINT or 143 for SIGTERM. A second SIGINT or SIGTERM ends the
//! process at once with the code of that signal, without a result.
//!
//! The handlers only store to atomic flags, or end the process, as
//! `signal-hook` arranges; nothing else runs inside them.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use crate::xlsx_write::{EXIT_SIGNAL_INT, EXIT_SIGNAL_TERM};

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
        for (signal, code) in [(SIGINT, EXIT_SIGNAL_INT), (SIGTERM, EXIT_SIGNAL_TERM)] {
            // Actions run in the order they are registered: a signal that
            // finds the flag already set ends the process; otherwise it
            // records which signal came and sets the flag.
            flag::register_conditional_shutdown(
                signal,
                i32::from(code),
                Arc::clone(&signals.stop),
            )?;
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
        if self.terminate.load(Ordering::SeqCst) {
            EXIT_SIGNAL_TERM
        } else {
            EXIT_SIGNAL_INT
        }
    }
}
