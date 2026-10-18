//! Recordings of raw reports, for replaying a session without a controller.
//!
//! A recording is a text file: a header line, then one line per report with the time since the
//! recording started (seconds), the hub slot it came from and its bytes in hex, report id first:
//!
//! ```text
//! # steam-controller-toolkit recording v1
//! 0.000000 1304:2 42a3010000...
//! 0.003781 1304:2 42a4010000...
//! ```

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

pub const HEADER: &str = "# steam-controller-toolkit recording v1";
pub const EXTENSION: &str = "screc";

/// One report as it arrived from the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// Time since the recording started.
    pub at: Duration,
    /// Hub slot, as in [`crate::HubEvent`].
    pub slot: String,
    /// Report bytes, report id first.
    pub data: Vec<u8>,
}

/// Writes packets to a recording as they arrive.
pub struct Writer<W: Write> {
    out: W,
}

impl<W: Write> Writer<W> {
    pub fn new(mut out: W) -> io::Result<Self> {
        writeln!(out, "{HEADER}")?;
        Ok(Self { out })
    }

    pub fn write(&mut self, at: Duration, slot: &str, data: &[u8]) -> io::Result<()> {
        write!(self.out, "{:.6} {slot} ", at.as_secs_f64())?;
        for b in data {
            write!(self.out, "{b:02x}")?;
        }
        writeln!(self.out)
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

fn parse_line(line: &str) -> Option<Packet> {
    let mut fields = line.split_ascii_whitespace();
    let secs: f64 = fields.next()?.parse().ok()?;
    let slot = fields.next()?.to_string();
    let hex = fields.next()?;
    if fields.next().is_some() || hex.len() % 2 != 0 || !secs.is_finite() || secs < 0.0 {
        return None;
    }
    let data = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    if data.is_empty() {
        return None;
    }
    Some(Packet { at: Duration::from_secs_f64(secs), slot, data })
}

/// Read a whole recording. Packets come back in time order.
pub fn read(input: impl BufRead) -> Result<Vec<Packet>, String> {
    let mut lines = input.lines();
    match lines.next() {
        Some(Ok(h)) if h.trim_end() == HEADER => {}
        Some(Err(e)) => return Err(e.to_string()),
        _ => return Err("Not a Steam Controller Toolkit recording".into()),
    }
    let mut packets = Vec::new();
    for (n, line) in lines.enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        packets.push(parse_line(&line).ok_or_else(|| format!("Line {} is not a report", n + 2))?);
    }
    packets.sort_by_key(|p| p.at);
    Ok(packets)
}

pub fn read_file(path: &Path) -> Result<Vec<Packet>, String> {
    read(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
}

/// Length of a recording, from its last line, without reading the whole file.
pub fn duration(path: &Path) -> io::Result<Duration> {
    let mut file = File::open(path)?;
    let len = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(len.saturating_sub(1024)))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    Ok(tail.lines().rev().find_map(parse_line).map_or(Duration::ZERO, |p| p.at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut w = Writer::new(Vec::new()).unwrap();
        w.write(Duration::from_micros(0), "1304:2", &[0x42, 0x01, 0xff]).unwrap();
        w.write(Duration::from_micros(3781), "1302:0", &[0x43, 0x00]).unwrap();
        let bytes = w.finish().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text, format!("{HEADER}\n0.000000 1304:2 4201ff\n0.003781 1302:0 4300\n"));

        let packets = read(bytes.as_slice()).unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[1], Packet { at: Duration::from_micros(3781), slot: "1302:0".into(), data: vec![0x43, 0x00] });
    }

    #[test]
    fn rejects_other_files() {
        assert!(read("hello\n".as_bytes()).is_err());
        assert!(read(format!("{HEADER}\n0.1 1304:2 4g\n").as_bytes()).is_err());
        assert!(read(format!("{HEADER}\n0.1 1304:2\n").as_bytes()).is_err());
    }
}
