use clap::Parser;
use console::{set_colors_enabled, Style};
use libc;
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::time::Instant;

#[derive(Parser)]
#[command(author, version, about = "Show a preview of the directory contents.")]
struct Args {
    /// Maximum number of lines to display
    #[arg(short = 'l', long, default_value_t = 2)]
    max_lines: usize,

    /// Directory to list
    #[arg(default_value = ".")]
    directory_path: String,
}

const MIN_TAB_WIDTH: u16 = 8;
const TIME_LIMIT_NS: u128 = 10_000_000; // 10ms

fn main() -> std::io::Result<()> {
    set_colors_enabled(true); // Force color output even when piping

    let args = Args::parse();
    let max_lines = args.max_lines;
    if max_lines == 0 {
        eprintln!("Error: `max_lines` must be greater than 0");
        std::process::exit(1);
    }

    let terminal_width = get_terminal_width();
    let max_columns = terminal_width
        .map(|width| (width / MIN_TAB_WIDTH) as usize)
        .unwrap_or(0)
        .max(1);

    let max_items = max_columns * max_lines;

    let mut times = vec![];
    let mut entries = vec![];
    let mut now = Instant::now();
    let mut num_dirs = 0;
    let mut too_many_dirs = false;
    let mut ran_out_of_time_listing = false;

    let mut i = 0;
    for entry in fs::read_dir(&args.directory_path)? {
        let entry_ref = entry.as_ref().unwrap();
        let name = entry_ref.file_name();
        if name.to_string_lossy().starts_with(".") {
            continue;
        }
        let meta = entry_ref.file_type()?;
        if meta.is_dir() {
            num_dirs += 1;
        }
        entries.push((name, meta));
        if num_dirs >= max_items {
            too_many_dirs = true;
            break;
        }
        let elapsed = now.elapsed().as_nanos();
        now = Instant::now();
        times.push((i, elapsed));
        if elapsed > TIME_LIMIT_NS {
            ran_out_of_time_listing = true;
            break;
        }
        i += 1;
    }

    let must_print_dirs = ran_out_of_time_listing || too_many_dirs || entries.len() > max_items;

    // Collect and sort entries by name
    let entries: Vec<_> = if must_print_dirs {
        entries
            .iter()
            .filter(|(_, meta)| meta.is_dir())
            .map(|(name, meta)| {
                let width = console::measure_text_width(&name.to_string_lossy()) as u16;
                (name, meta, width)
            })
            .collect()
    } else {
        entries
            .iter()
            .map(|(name, meta)| {
                let width = console::measure_text_width(&name.to_string_lossy()) as u16;
                (name, meta, width)
            })
            .collect()
    };

    if entries.is_empty() {
        return Ok(());
    }

    print_entries_in_columns(entries, terminal_width, max_lines, !must_print_dirs)?;

    Ok(())
}

fn print_entries_in_columns(
    mut entries: Vec<(&std::ffi::OsString, &std::fs::FileType, u16)>,
    terminal_width: Option<u16>,
    max_lines: usize,
    consider_dirs_only: bool,
) -> io::Result<()> {
    // Find the maximum width needed
    let max_width = entries.iter().map(|(_, _, width)| *width).max().unwrap();

    // Round up to next multiple of 8
    let column_width = (((max_width - 1) / 8) + 1) * 8;

    let num_columns = std::cmp::max(1, terminal_width.map(|w| w / column_width).unwrap_or(0));

    if consider_dirs_only {
        let max_items = (num_columns as usize) * max_lines;
        if entries.len() > max_items {
            let is_dir = |(_, meta, _): &(_, &std::fs::FileType, _)| meta.is_dir();
            if entries.iter().any(is_dir) {
                return print_entries_in_columns(
                    entries.into_iter().filter(is_dir).collect(),
                    terminal_width,
                    max_lines,
                    false
                );
            }
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Print entries in columns
    for (i, (name, file_type, width)) in entries.iter().enumerate() {
        if i == max_lines * (num_columns as usize) - 1 {
            println!("....");
            break;
        }

        let (style, indicator) = get_color_and_indicator(file_type);
        print!("{}{}", style.apply_to(name.to_string_lossy()), indicator);

        // Add padding to align to column width, except for last column
        let next_row_index = (i as u16 + 1) % num_columns;
        if next_row_index != 0 {
            let padding = column_width - *width;
            print!("{}", " ".repeat(padding as usize));
        }

        // New line after each row
        if next_row_index == 0 || i == entries.len() - 1 {
            println!();
        }
    }
    Ok(())
}

fn get_color_and_indicator(file_type: &fs::FileType) -> (Style, &'static str) {
    if file_type.is_dir() {
        (Style::new().blue().bold(), "/")
    } else if file_type.is_symlink() {
        (Style::new().magenta(), "@")
    } else if file_type.is_socket() {
        (Style::new().magenta(), "=")
    } else if file_type.is_fifo() {
        (Style::new().yellow(), "|")
    } else if file_type.is_block_device() {
        (Style::new().blue().on_cyan(), "")
    } else if file_type.is_char_device() {
        (Style::new().blue().on_yellow(), "")
    } else if file_type.is_file() {
        (Style::new(), "")
    } else {
        (Style::new(), "")
    }
}

fn get_terminal_width() -> Option<u16> {
    let tty = std::fs::File::open("/dev/tty").ok()?;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };
    if result == 0 && ws.ws_col > 0 {
        Some(ws.ws_col)
    } else {
        None
    }
}
