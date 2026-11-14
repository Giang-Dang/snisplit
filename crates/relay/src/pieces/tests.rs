use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::AsyncWrite;

use super::*;

const NO_GAP: Duration = Duration::ZERO;
const GAP: Duration = Duration::from_millis(60);
const GAP_LOWER_BOUND: Duration = Duration::from_millis(50);
const UNLIMITED: usize = usize::MAX;
const NEVER: usize = usize::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Write(Vec<u8>),
    Flush,
}

struct Recorder {
    ops: Vec<Op>,
    write_times: Vec<Instant>,
    accepts_at_most: usize,
    fails_on_write: usize,
}

impl Recorder {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            write_times: Vec::new(),
            accepts_at_most: UNLIMITED,
            fails_on_write: NEVER,
        }
    }

    fn writes(&self) -> Vec<Vec<u8>> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                Op::Write(bytes) => Some(bytes.clone()),
                Op::Flush => None,
            })
            .collect()
    }
}

impl AsyncWrite for Recorder {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.write_times.len() == this.fails_on_write {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let accepted = buf.len().min(this.accepts_at_most);
        this.ops.push(Op::Write(buf[..accepted].to_vec()));
        this.write_times.push(Instant::now());
        Poll::Ready(Ok(accepted))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().ops.push(Op::Flush);
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

fn write_all_pieces(
    recorder: &mut Recorder,
    bytes: &[u8],
    cuts: &[usize],
    gap: Duration,
) -> io::Result<usize> {
    block_on(write_pieces(recorder, bytes, cuts, gap))
}

#[test]
fn every_piece_is_its_own_write_followed_by_a_flush() {
    let mut recorder = Recorder::new();
    let pieces = write_all_pieces(&mut recorder, b"ABCDEFGH", &[1, 5], NO_GAP).unwrap();
    assert_eq!(pieces, 3);
    assert_eq!(
        recorder.ops,
        [
            Op::Write(b"A".to_vec()),
            Op::Flush,
            Op::Write(b"BCDE".to_vec()),
            Op::Flush,
            Op::Write(b"FGH".to_vec()),
            Op::Flush,
        ]
    );
}

#[test]
fn the_gap_separates_consecutive_pieces() {
    let mut recorder = Recorder::new();
    write_all_pieces(&mut recorder, b"ABCDEFGH", &[1, 5], GAP).unwrap();
    assert_eq!(recorder.write_times.len(), 3);
    for pair in recorder.write_times.windows(2) {
        assert!(
            pair[1] - pair[0] >= GAP_LOWER_BOUND,
            "pieces were written {:?} apart",
            pair[1] - pair[0]
        );
    }
}

#[test]
fn no_cuts_is_one_write_and_no_bytes_is_no_write() {
    let mut recorder = Recorder::new();
    assert_eq!(
        write_all_pieces(&mut recorder, b"ABCDEFGH", &[], GAP).unwrap(),
        1
    );
    assert_eq!(recorder.writes(), [b"ABCDEFGH".to_vec()]);

    let mut empty = Recorder::new();
    assert_eq!(write_all_pieces(&mut empty, b"", &[], GAP).unwrap(), 0);
    assert_eq!(empty.ops, []);
}

#[test]
fn cuts_that_are_zero_repeated_unsorted_or_past_the_end_make_no_empty_writes() {
    let mut recorder = Recorder::new();
    let pieces =
        write_all_pieces(&mut recorder, b"ABCDEFGH", &[0, 5, 5, 3, 100, 8], NO_GAP).unwrap();
    assert_eq!(pieces, 2);
    assert_eq!(recorder.writes(), [b"ABCDE".to_vec(), b"FGH".to_vec()]);
}

#[test]
fn a_writer_that_takes_few_bytes_per_call_still_gets_every_byte_in_order() {
    let mut recorder = Recorder::new();
    recorder.accepts_at_most = 3;
    let pieces = write_all_pieces(&mut recorder, b"ABCDEFGH", &[4], NO_GAP).unwrap();
    assert_eq!(pieces, 2);
    assert_eq!(recorder.writes().concat(), b"ABCDEFGH");
    assert!(recorder.writes().iter().all(|write| write.len() <= 3));
}

#[test]
fn a_failing_write_is_returned() {
    let mut recorder = Recorder::new();
    recorder.fails_on_write = 1;
    let error = write_all_pieces(&mut recorder, b"ABCDEFGH", &[1, 5], NO_GAP).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(recorder.writes(), [b"A".to_vec()]);
}
