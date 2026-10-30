//! The serial port: opening it, listing the ports and the two system
//! threads that turn its blocking I/O into channels (spec m8-10, D2702,
//! D2703, D2705, D2706).

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};
use tokio::sync::mpsc;

use crate::error::EboardError;
use crate::line::{Line, LineSplitter};
use crate::novag::BAUD_RATE;

/// How long a read waits before looking at the stop flag (D2705).
pub const READ_POLL: Duration = Duration::from_millis(100);

/// The longest port name accepted.
const MAX_PORT_BYTES: usize = 255;

/// The two halves of an open device, ready for the session threads.
pub struct DeviceIo {
    /// Reads must return `TimedOut` (or `WouldBlock`) instead of blocking
    /// forever, so the thread can see the stop flag.
    pub reader: Box<dyn Read + Send>,
    /// Writes of whole lines.
    pub writer: Box<dyn Write + Send>,
}

impl std::fmt::Debug for DeviceIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceIo").finish_non_exhaustive()
    }
}

/// A serial port found on the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// The name to open.
    pub name: String,
    /// A short description (USB product, Bluetooth, PCI), possibly empty.
    pub description: String,
}

/// Checks a port name (D2703): not empty, at most 255 bytes, no NUL and no
/// line break. It does not check that the port exists.
///
/// # Errors
///
/// [`EboardError::InvalidPort`].
pub fn validate_port(port: &str) -> Result<(), EboardError> {
    let bad = |why: &str| Err(EboardError::InvalidPort(why.to_owned()));
    if port.trim().is_empty() {
        return bad("empty");
    }
    if port.len() > MAX_PORT_BYTES {
        return bad("too long");
    }
    if port.chars().any(|c| matches!(c, '\0' | '\n' | '\r')) {
        return bad("control characters");
    }
    Ok(())
}

/// Opens the port at 57600 8N1 without flow control (D2704). Blocking: call
/// it from `spawn_blocking`.
///
/// # Errors
///
/// [`EboardError::InvalidPort`] or [`EboardError::OpenPort`] with the message
/// of the system.
pub fn open_serial(port: &str) -> Result<DeviceIo, EboardError> {
    validate_port(port)?;
    let port = port.trim();
    let open_error = |e: &dyn std::fmt::Display| EboardError::OpenPort {
        port: port.to_owned(),
        message: e.to_string(),
    };
    // The library is third-party code on hardware paths: a panic in it must
    // not take the application down.
    let opened = catch_unwind(AssertUnwindSafe(|| {
        serialport::new(port, BAUD_RATE)
            .data_bits(DataBits::Eight)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .flow_control(FlowControl::None)
            .timeout(READ_POLL)
            .open()
    }))
    .map_err(|_| open_error(&"the serial library failed"))?
    .map_err(|e| open_error(&e))?;
    let writer = opened.try_clone().map_err(|e| open_error(&e))?;
    Ok(DeviceIo {
        reader: Box::new(opened),
        writer: Box::new(writer),
    })
}

/// Lists the serial ports of the system (D2702). Blocking; the library
/// call is guarded against panics.
///
/// # Errors
///
/// [`EboardError::PortList`].
pub fn list_ports() -> Result<Vec<PortInfo>, EboardError> {
    let found = catch_unwind(serialport::available_ports)
        .map_err(|_| EboardError::PortList("the serial library failed".to_owned()))?
        .map_err(|e| EboardError::PortList(e.to_string()))?;
    let mut ports: Vec<PortInfo> = found
        .into_iter()
        .map(|p| PortInfo {
            description: describe(&p.port_type),
            name: p.port_name,
        })
        .collect();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    ports.dedup_by(|a, b| a.name == b.name);
    Ok(ports)
}

fn describe(kind: &serialport::SerialPortType) -> String {
    use serialport::SerialPortType as T;
    match kind {
        T::UsbPort(usb) => usb
            .product
            .clone()
            .or_else(|| usb.manufacturer.clone())
            .unwrap_or_else(|| "USB".to_owned()),
        T::BluetoothPort => "Bluetooth".to_owned(),
        T::PciPort => "PCI".to_owned(),
        T::Unknown => String::new(),
    }
}

/// What the reader thread reports.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IoEvent {
    /// A complete line.
    Line(Line),
    /// The device failed or went away; the text is for the user.
    Lost(String),
}

/// The threads of one open device. Dropping it stops them.
pub(crate) struct DeviceThreads {
    pub(crate) events: mpsc::Receiver<IoEvent>,
    writer: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

/// How long [`DeviceThreads::shutdown`] waits for the threads to end.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// How many times in a row a write may time out before the device counts as
/// lost (about two seconds with the 100 ms of the port).
const MAX_WRITE_STALLS: u32 = 20;

/// `write_all` + `flush` that tries again when the port only timed out
/// (a busy adapter is not a lost one), a limited number of times.
fn write_with_retry(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind::{Interrupted, TimedOut, WouldBlock};
    let mut stalls = 0_u32;
    let mut sent = 0;
    let mut flushed = false;
    while !(flushed && sent == bytes.len()) {
        let step = if sent < bytes.len() {
            match writer.write(&bytes[sent..]) {
                Ok(0) => Err(std::io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    sent += n;
                    stalls = 0;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            writer.flush().map(|()| flushed = true)
        };
        match step {
            Ok(()) => {}
            Err(e) if e.kind() == Interrupted => {}
            Err(e) if matches!(e.kind(), TimedOut | WouldBlock) => {
                stalls += 1;
                if stalls > MAX_WRITE_STALLS {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl DeviceThreads {
    /// Queues bytes for the writer thread. `false` if the thread is gone.
    pub(crate) fn write(&self, bytes: Vec<u8>) -> bool {
        self.writer.as_ref().is_some_and(|w| w.send(bytes).is_ok())
    }

    /// Stops the threads and waits for them to end (bounded), so the port
    /// is released when this returns; a thread stuck in a write is left
    /// behind after the wait.
    pub(crate) async fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.writer = None;
        // A reader stuck sending to a queue nobody reads must wake up.
        self.events.close();
        let handles = std::mem::take(&mut self.handles);
        let joined = tokio::task::spawn_blocking(move || {
            for handle in handles {
                handle.join().ok();
            }
        });
        tokio::time::timeout(SHUTDOWN_WAIT, joined).await.ok();
    }
}

impl Drop for DeviceThreads {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.writer = None;
    }
}

/// Starts the reader and writer threads of a device (D2705).
pub(crate) fn spawn_io(device: DeviceIo) -> DeviceThreads {
    let (tx, rx) = mpsc::channel(crate::EVENT_QUEUE);
    let stop = Arc::new(AtomicBool::new(false));
    let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<u8>>();

    let DeviceIo {
        mut reader,
        mut writer,
    } = device;

    let mut handles = Vec::new();
    let reader_tx = tx.clone();
    let reader_stop = Arc::clone(&stop);
    let spawned = std::thread::Builder::new()
        .name("eboard-read".to_owned())
        .spawn(move || {
            let mut splitter = LineSplitter::new();
            let mut buf = [0_u8; 256];
            let mut lines = Vec::new();
            while !reader_stop.load(Ordering::SeqCst) {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        reader_tx
                            .blocking_send(IoEvent::Lost("end of data".to_owned()))
                            .ok();
                        return;
                    }
                    Ok(n) => {
                        splitter.feed(&buf[..n], &mut lines);
                        for line in lines.drain(..) {
                            if reader_tx.blocking_send(IoEvent::Line(line)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => {
                        reader_tx.blocking_send(IoEvent::Lost(e.to_string())).ok();
                        return;
                    }
                }
            }
        });
    match spawned {
        Ok(handle) => handles.push(handle),
        Err(_) => {
            tx.try_send(IoEvent::Lost("cannot start the reader thread".to_owned()))
                .ok();
        }
    }

    let writer_tx = tx;
    let spawned = std::thread::Builder::new()
        .name("eboard-write".to_owned())
        .spawn(move || {
            while let Ok(bytes) = write_rx.recv() {
                if let Err(e) = write_with_retry(&mut writer, &bytes) {
                    writer_tx.blocking_send(IoEvent::Lost(e.to_string())).ok();
                    return;
                }
            }
        });
    if let Ok(handle) = spawned {
        handles.push(handle);
    } else {
        // The sender below is dropped with the struct; writes report failure.
        tracing::warn!("cannot start the eboard writer thread");
    }

    DeviceThreads {
        events: rx,
        writer: Some(write_tx),
        stop,
        handles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_names_are_checked_loosely() {
        for ok in [
            "COM3",
            "COM12",
            r"\\.\COM10",
            "/dev/ttyUSB0",
            "/dev/cu.usbserial-1410",
        ] {
            assert!(validate_port(ok).is_ok(), "{ok}");
        }
        for bad in ["", "   ", "a\0b", "a\nb", &"x".repeat(300)] {
            assert!(matches!(
                validate_port(bad),
                Err(EboardError::InvalidPort(_))
            ));
        }
    }

    #[test]
    fn opening_a_missing_port_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join("mychess-no-such-port");
        let err = open_serial(&missing.display().to_string()).expect_err("must fail");
        assert!(matches!(err, EboardError::OpenPort { .. }), "{err:?}");
        assert!(matches!(
            open_serial("a\0b"),
            Err(EboardError::InvalidPort(_))
        ));
    }

    #[test]
    fn listing_ports_does_not_panic() {
        // The answer depends on the machine; only the absence of a panic and
        // the ordering are checked.
        if let Ok(ports) = list_ports() {
            assert!(ports.windows(2).all(|w| w[0].name < w[1].name));
        }
    }
}
