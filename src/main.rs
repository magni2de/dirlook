mod app;
mod color;
mod key;
mod node;
mod scanner;
mod screen;
mod term;
mod treemap;
mod ui;
mod width;

use std::io::Write;
use std::path::PathBuf;

fn main() {
    let mut path: Option<PathBuf> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-V" | "-v" | "-version" | "--version" => {
                print_version();
                return;
            }
            s if s.starts_with('-') && s != "-" => {
                eprintln!("dirlook: unknown option '{s}'");
                eprintln!("Try 'dirlook --help' for more information.");
                std::process::exit(2);
            }
            _ => {
                if path.is_none() {
                    path = Some(PathBuf::from(arg));
                }
            }
        }
    }
    let path = path.unwrap_or_else(|| PathBuf::from("."));

    let saved = match term::set_raw_mode(0) {
        Some(t) => t,
        None => {
            eprintln!("Failed to enter raw mode");
            std::process::exit(1);
        }
    };

    let mut app = app::App::new(path);
    app.cell_width = term::detect_cell_width();

    print!("\x1b[?1049h\x1b[?25l");
    let mut stdout = std::io::stdout();
    let _ = stdout.flush();

    let mut screen = screen::Screen::new();

    'ui: loop {
        app.tick();
        app.pump();
        ui::render(&app, &mut screen);

        match key::read_key_timeout(50) {
            key::KeyEvent::Key(k) => app.handle_key(k),
            key::KeyEvent::Timeout => {}
            key::KeyEvent::Eof => break 'ui,
        }

        if app.quit {
            break 'ui;
        }
    }

    print!("\x1b[?25h\x1b[?1049l");
    let mut stdout = std::io::stdout();
    let _ = stdout.flush();
    term::restore_termios(0, &saved);
}

fn print_version() {
    println!("dirlook v{}", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "dirlook — terminal disk usage analyzer

USAGE:
    dirlook [OPTIONS] [PATH]

ARGS:
    <PATH>    Directory to analyze (default: current directory)

OPTIONS:
    -h, --help        Print this help
    -V, --version     Print version

KEYS:
    Up/Down, j/k       Move selection
    Right              Expand / collapse
    Enter              Enter directory (or go to parent on '..')
    Left, Backspace    Collapse / move to parent
    s                  Cycle sort (name / size desc / size asc)
    [ / ]              Move the tree/map divider (left/right side-by-side, up/down stacked)
    m                  Toggle layout (side-by-side / stacked)
    /                  Toggle color legend
    q                  Quit"
    );
}
