use std::io::{IsTerminal, Read, Write};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::render::CellPixels;

const QUERY_CELL_SIZE_PIXELS: &[u8] = b"\x1b[16t";
const QUERY_TEXT_AREA_PIXELS: &[u8] = b"\x1b[14t";
const QUERY_TEXT_AREA_CELLS: &[u8] = b"\x1b[18t";

const REPORT_WINDOW_PIXELS: u16 = 4;
const REPORT_TEXT_AREA_PIXELS: u16 = 5;
const REPORT_CELL_SIZE_PIXELS: u16 = 6;
const REPORT_TEXT_AREA_CELLS: u16 = 8;

static QUERIED_CELL_PIXELS: OnceLock<Option<CellPixels>> = OnceLock::new();

/// Resolve the terminal cell size in pixels once per process.
///
/// Terminals that fill `TIOCGWINSZ` already answer this for free, so the escape
/// round trip only happens when that ioctl leaves the pixel fields empty, which
/// is common for terminal multiplexers and for terminals reached over a
/// transport that does not forward the ioctl data.
pub fn prime_cell_pixels(timeout: Duration) -> Option<CellPixels> {
    *QUERIED_CELL_PIXELS.get_or_init(|| {
        if ioctl_reports_pixels() {
            return None;
        }
        query_cell_pixels(timeout)
    })
}

pub fn cached_cell_pixels() -> Option<CellPixels> {
    QUERIED_CELL_PIXELS.get().copied().flatten()
}

fn ioctl_reports_pixels() -> bool {
    crossterm::terminal::window_size()
        .map(|window| window.width > 0 && window.height > 0)
        .unwrap_or(false)
}

/// Ask the terminal for its cell size, falling back to deriving it from the
/// text area size in pixels and in cells.
fn query_cell_pixels(timeout: Duration) -> Option<CellPixels> {
    if !std::io::stdin().is_terminal() {
        return None;
    }

    let guard = RawModeGuard::enable()?;
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let direct = ask(&mut writer, QUERY_CELL_SIZE_PIXELS, timeout)
        .and_then(|reply| parse_cell_size_pixels(&reply));
    let cell = direct.or_else(|| {
        let pixels = ask(&mut writer, QUERY_TEXT_AREA_PIXELS, timeout)
            .and_then(|reply| parse_text_area_pixels(&reply));
        let cells = ask(&mut writer, QUERY_TEXT_AREA_CELLS, timeout)
            .and_then(|reply| parse_text_area_cells(&reply));
        let (width_px, height_px) = pixels?;
        let (columns, rows) = cells?;
        cell_pixels_from_text_area(width_px, height_px, columns, rows)
    });

    drop(guard);
    cell
}

fn ask(writer: &mut impl Write, query: &[u8], timeout: Duration) -> Option<String> {
    writer.write_all(query).ok()?;
    writer.flush().ok()?;
    read_reply(timeout)
}

fn read_reply(timeout: Duration) -> Option<String> {
    let mut stdin = std::io::stdin();
    let mut reply = String::new();
    let mut buffer = [0u8; 64];
    let deadline = Instant::now() + timeout;

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if !input_is_ready(remaining) {
            break;
        }
        match stdin.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => reply.push_str(&String::from_utf8_lossy(&buffer[..read])),
        }
        if reply.contains('t') {
            break;
        }
    }

    (!reply.is_empty()).then_some(reply)
}

fn input_is_ready(timeout: Duration) -> bool {
    let mut descriptor = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let milliseconds = timeout.as_millis().min(i32::MAX as u128) as i32;

    // SAFETY: `descriptor` points at a single initialized pollfd that stays alive
    // for the whole call, and the count matches the pointer.
    unsafe { libc::poll(&mut descriptor, 1, milliseconds) > 0 }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Option<Self> {
        crossterm::terminal::enable_raw_mode().ok().map(|()| Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Parse a `CSI <report> ; <height> ; <width> t` reply as documented by xterm.
fn parse_report(reply: &str, report: u16) -> Option<(u16, u16)> {
    reply
        .split('\x1b')
        .filter_map(|part| part.strip_prefix('['))
        .find_map(|parameters| {
            let parameters = parameters.strip_suffix('t')?;
            let mut fields = parameters.split(';');
            if fields.next()?.parse::<u16>().ok()? != report {
                return None;
            }
            let height = fields.next()?.parse().ok()?;
            let width = fields.next()?.parse().ok()?;
            Some((height, width))
        })
}

/// Reply of `CSI 16 t`: the character cell size in pixels.
pub fn parse_cell_size_pixels(reply: &str) -> Option<CellPixels> {
    let (height, width) = parse_report(reply, REPORT_CELL_SIZE_PIXELS)?;
    nonzero_cell(width, height)
}

/// Reply of `CSI 14 t` or `CSI 5 t`: the text area size in pixels.
pub fn parse_text_area_pixels(reply: &str) -> Option<(u16, u16)> {
    parse_report(reply, REPORT_WINDOW_PIXELS)
        .or_else(|| parse_report(reply, REPORT_TEXT_AREA_PIXELS))
        .map(|(height, width)| (width, height))
}

/// Reply of `CSI 18 t`: the text area size in character cells.
pub fn parse_text_area_cells(reply: &str) -> Option<(u16, u16)> {
    parse_report(reply, REPORT_TEXT_AREA_CELLS).map(|(height, width)| (width, height))
}

/// Derive the cell size from the text area size in pixels and in cells.
pub fn cell_pixels_from_text_area(
    width_px: u16,
    height_px: u16,
    columns: u16,
    rows: u16,
) -> Option<CellPixels> {
    if columns == 0 || rows == 0 {
        return None;
    }

    nonzero_cell(width_px / columns, height_px / rows)
}

fn nonzero_cell(width: u16, height: u16) -> Option<CellPixels> {
    (width > 0 && height > 0).then_some(CellPixels { width, height })
}
