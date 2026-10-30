//! Input source: plain bytes or a gzip stream, told apart by the magic
//! bytes `1F 8B` (D30), read lazily.

use std::io::{self, Chain, Cursor, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::read::MultiGzDecoder;

/// The two bytes that start every gzip member.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Remembers whether the underlying reader returned an error, to tell a
/// failing file from a corrupt gzip stream.
struct Watch<R> {
    inner: R,
    failed: Arc<AtomicBool>,
}

impl<R: Read> Read for Watch<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(out);
        if matches!(&result, Err(e) if e.kind() != io::ErrorKind::Interrupted) {
            self.failed.store(true, Ordering::Relaxed);
        }
        result
    }
}

type Replay<R> = Chain<Cursor<Vec<u8>>, Watch<R>>;

enum State<R: Read> {
    /// Nothing read yet.
    Fresh(Watch<R>),
    Plain(Replay<R>),
    Gzip(Box<MultiGzDecoder<Replay<R>>>),
    /// A previous initialisation failed.
    Poisoned,
}

/// Reads decompressed bytes from `R`, decompressing when it holds gzip data
/// (concatenated members included). There is no limit on the compression
/// ratio: a hostile `.gz` costs CPU time (not memory, the output is read in
/// bounded chunks and games are bounded by `max_game_bytes`).
pub(crate) struct Input<R: Read> {
    state: State<R>,
    gzip: bool,
    failed: Arc<AtomicBool>,
}

impl<R: Read> Input<R> {
    pub(crate) fn new(inner: R) -> Input<R> {
        let failed = Arc::new(AtomicBool::new(false));
        Input {
            state: State::Fresh(Watch {
                inner,
                failed: Arc::clone(&failed),
            }),
            gzip: false,
            failed,
        }
    }

    /// Whether the underlying reader (the file) failed, as opposed to the
    /// gzip decoder rejecting its data.
    pub(crate) fn source_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Whether the stream turned out to be gzip (known after the first read).
    pub(crate) fn is_gzip(&self) -> bool {
        self.gzip
    }

    fn init(&mut self) -> io::Result<()> {
        let State::Fresh(mut inner) = std::mem::replace(&mut self.state, State::Poisoned) else {
            return Ok(());
        };
        let mut magic = [0u8; 2];
        let mut got = 0;
        while got < magic.len() {
            match inner.read(&mut magic[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        let replay = Cursor::new(magic[..got].to_vec()).chain(inner);
        self.state = if got == magic.len() && magic == GZIP_MAGIC {
            self.gzip = true;
            State::Gzip(Box::new(MultiGzDecoder::new(replay)))
        } else {
            State::Plain(replay)
        };
        Ok(())
    }
}

impl<R: Read> Read for Input<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if matches!(self.state, State::Fresh(_)) {
            self.init()?;
        }
        match &mut self.state {
            State::Plain(r) => r.read(out),
            State::Gzip(r) => r.read(out),
            State::Fresh(_) | State::Poisoned => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    fn read_all(bytes: &[u8]) -> (Vec<u8>, bool) {
        let mut input = Input::new(bytes);
        let mut out = Vec::new();
        input.read_to_end(&mut out).expect("read");
        (out, input.is_gzip())
    }

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(data).expect("write");
        enc.finish().expect("finish")
    }

    #[test]
    fn plain_data_passes_through() {
        assert_eq!(
            read_all(b"[Event \"x\"]"),
            (b"[Event \"x\"]".to_vec(), false)
        );
        assert_eq!(read_all(b""), (Vec::new(), false));
        assert_eq!(read_all(b"\x1f"), (vec![0x1f], false));
    }

    #[test]
    fn gzip_is_detected_by_magic_not_by_name() {
        let (out, is_gz) = read_all(&gz(b"[Event \"x\"]\n"));
        assert!(is_gz);
        assert_eq!(out, b"[Event \"x\"]\n");
    }

    #[test]
    fn concatenated_members_are_read_in_sequence() {
        let mut data = gz(b"one\n");
        data.extend(gz(b"two\n"));
        assert_eq!(read_all(&data).0, b"one\ntwo\n");
    }

    #[test]
    fn corrupt_gzip_is_an_error() {
        let mut data = gz(&vec![b'a'; 100_000]);
        data.truncate(data.len() / 2);
        let mut input = Input::new(&data[..]);
        let mut out = Vec::new();
        assert!(input.read_to_end(&mut out).is_err());
        assert!(input.is_gzip());
    }
}
