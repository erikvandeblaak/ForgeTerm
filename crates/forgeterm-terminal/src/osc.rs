//! Finds complete OSC sequences (`ESC ] payload BEL` or `ESC ] payload ESC \`) in raw
//! PTY output. The emulator ignores the OSCs ForgeTerm cares about (working
//! directory, shell-integration marks, program status), so the session taps the
//! byte stream with this scanner before the emulator sees it.

/// Longest OSC payload that is considered; longer sequences are ignored (OSC 7501
/// caps a whole report at 4096 bytes too).
pub(crate) const MAX_PAYLOAD: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Osc,
    OscEsc,
}

/// Incremental scanner; sequences may be split across reads.
#[derive(Debug, Clone)]
pub(crate) struct OscScanner {
    state: State,
    payload: Vec<u8>,
    overflow: bool,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            payload: Vec::new(),
            overflow: false,
        }
    }
}

impl OscScanner {
    /// Feeds output bytes; calls `on_payload` for every complete OSC payload, in
    /// order. Oversized and aborted sequences are skipped.
    pub(crate) fn feed(&mut self, bytes: &[u8], mut on_payload: impl FnMut(&[u8])) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Ground, 0x1b) => State::Esc,
                (State::Ground, _) => State::Ground,
                (State::Esc, b']') => {
                    self.start();
                    State::Osc
                }
                (State::Esc, 0x1b) => State::Esc,
                (State::Esc, _) => State::Ground,
                (State::Osc, 0x07) => {
                    self.finish(&mut on_payload);
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEsc,
                // CAN / SUB abort a control sequence.
                (State::Osc, 0x18 | 0x1a) => State::Ground,
                (State::Osc, _) => {
                    self.push(b);
                    State::Osc
                }
                (State::OscEsc, b'\\') => {
                    self.finish(&mut on_payload);
                    State::Ground
                }
                // ESC followed by something else: the OSC is cut off, a new escape starts.
                (State::OscEsc, b']') => {
                    self.start();
                    State::Osc
                }
                (State::OscEsc, _) => State::Ground,
            };
        }
    }

    fn start(&mut self) {
        self.payload.clear();
        self.overflow = false;
    }

    fn push(&mut self, b: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, on_payload: &mut impl FnMut(&[u8])) {
        if !self.overflow {
            on_payload(&self.payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payloads(chunks: &[&[u8]]) -> Vec<String> {
        let mut s = OscScanner::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, |p| out.push(String::from_utf8_lossy(p).into_owned()));
        }
        out
    }

    #[test]
    fn both_terminators_in_order_and_split() {
        assert_eq!(
            payloads(&[b"a\x1b]133;A\x07b\x1b]8;;x\x1b", b"\\c\x1b]7501;?\x1b\\"]),
            ["133;A", "8;;x", "7501;?"]
        );
    }

    #[test]
    fn aborted_and_oversized_are_skipped() {
        let mut long = b"\x1b]".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_PAYLOAD + 1));
        long.push(0x07);
        assert_eq!(
            payloads(&[b"\x1b]133;A\x18\x1b]133;B\x07", &long, b"\x1b]0;t\x07"]),
            ["133;B", "0;t"]
        );
        // An OSC cut off by a new one.
        assert_eq!(payloads(&[b"\x1b]0;x\x1b]133;C\x07"]), ["133;C"]);
    }
}
