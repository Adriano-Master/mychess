//! The command line of the `mychess` executable (spec M9-4, D4040).
//!
//! `mychess [OPTIONS] [FILE]...`. The files are opened once the window is up
//! (databases, EPD files, Polyglot books). From the options of the Scid
//! (`tcl/end.tcl`, `getCommandLineOptions`) only the ones that mean something
//! here are kept: see `docs/specs/m9-4-distribuicao-e-primeira-execucao.md`.

use std::path::PathBuf;

use clap::Parser;
use mychess_app::start::{
    LaunchFile, MAX_LAUNCH_FILES, StartInfo, UserLayout, classify_launch_path, detect_inputs,
    is_writable_dir, resolve_layout,
};

/// What `mychess --help` shows.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(
    name = "mychess",
    version,
    about = "MyChess: a chess database and analysis program.",
    after_help = "Files: databases (.si4, .pgn and any other text file read as PGN), EPD files (.epd) and Polyglot books (.bin). At most eight files are opened.\n\nEnvironment: MYCHESS_HOME replaces the home folder that holds the MyChess folder."
)]
pub struct Cli {
    /// Portable mode: keep the preferences, the data and the MyChess folder
    /// next to the executable (a `portable.flag` file there does the same).
    #[arg(long)]
    pub portable: bool,
    /// Do not show the tip of the day this time.
    #[arg(long = "no-tip")]
    pub no_tip: bool,
    /// Files to open.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,
}

impl Cli {
    /// Parses `args` (the first is the program name). `--help` and
    /// `--version` come back as the error kinds of clap that the caller
    /// prints with [`clap::Error::exit`].
    ///
    /// # Errors
    /// An unknown option or a missing value, and `--help`/`--version`.
    pub fn parse_args<I, T>(args: I) -> Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        Cli::try_parse_from(args)
    }
}

/// The files of the command line as the program opens them: made absolute
/// (the working directory may not be the one the user sees later), at most
/// [`MAX_LAUNCH_FILES`], repeated paths once.
#[must_use]
pub fn launch_files(paths: &[PathBuf]) -> Vec<LaunchFile> {
    let mut files: Vec<LaunchFile> = Vec::new();
    for path in paths {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        if files.iter().any(|f| f.path() == path) {
            continue;
        }
        files.push(classify_launch_path(&path));
        if files.len() == MAX_LAUNCH_FILES {
            break;
        }
    }
    files
}

/// What the application is told at launch.
#[must_use]
pub fn start_info(cli: &Cli, layout: &UserLayout) -> StartInfo {
    StartInfo {
        root: layout.root.clone(),
        mode: layout.mode,
        warnings: layout.warnings.clone(),
        files: launch_files(&cli.files),
        skip_tip: cli.no_tip,
    }
}

/// Reads the environment and chooses the folders (a few `stat` calls and,
/// in portable mode, the creation of the folder next to the executable).
/// Install `layout.dirs` with [`mychess_app::start::install_app_dirs`]
/// before anything asks for a path.
#[must_use]
pub fn resolve(cli: &Cli) -> UserLayout {
    resolve_layout(&detect_inputs(cli.portable), is_writable_dir)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::error::ErrorKind;

    use super::*;

    fn is_inside(path: &Path, root: &Path) -> bool {
        path.starts_with(root)
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::parse_args(std::iter::once("mychess").chain(args.iter().copied()))
    }

    #[test]
    fn no_arguments_is_a_plain_start() {
        let cli = parse(&[]).expect("parse");
        assert!(!cli.portable && !cli.no_tip && cli.files.is_empty());
    }

    #[test]
    fn options_and_files_mix() {
        let cli = parse(&["--portable", "a.si4", "--no-tip", "b.pgn"]).expect("parse");
        assert!(cli.portable && cli.no_tip);
        assert_eq!(cli.files, [PathBuf::from("a.si4"), PathBuf::from("b.pgn")]);
    }

    #[test]
    fn a_file_that_starts_with_a_dash_follows_the_separator() {
        let cli = parse(&["--", "-odd.pgn"]).expect("parse");
        assert_eq!(cli.files, [PathBuf::from("-odd.pgn")]);
    }

    #[test]
    fn unknown_options_are_errors_not_panics() {
        for bad in ["--bogus", "-fast", "--lang=x", "-s3"] {
            let error = parse(&[bad]).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::UnknownArgument, "{bad}");
        }
    }

    #[test]
    fn help_and_version_are_reported_as_such() {
        assert_eq!(
            parse(&["--help"]).expect_err("help").kind(),
            ErrorKind::DisplayHelp
        );
        let version = parse(&["--version"]).expect_err("version");
        assert_eq!(version.kind(), ErrorKind::DisplayVersion);
        assert!(version.to_string().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn help_lists_the_options_and_the_file_kinds() {
        let text = parse(&["--help"]).expect_err("help").to_string();
        for needle in ["--portable", "--no-tip", "FILE", ".si4", ".epd", ".bin"] {
            assert!(text.contains(needle), "{needle} in:\n{text}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_accepted() {
        use std::os::unix::ffi::OsStringExt;
        let odd = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'p', b'g', b'n']);
        let cli = Cli::parse_args([std::ffi::OsString::from("mychess"), odd.clone()]).expect("ok");
        assert_eq!(cli.files, [PathBuf::from(odd)]);
    }

    #[test]
    fn files_become_absolute_unique_and_classified() {
        let files = launch_files(&[
            PathBuf::from("é dir/Jogos.PGN"),
            PathBuf::from("é dir/Jogos.PGN"),
            PathBuf::from("pos.epd"),
            PathBuf::from("book.bin"),
            PathBuf::from("does-not-exist.si4"),
        ]);
        assert_eq!(files.len(), 4, "the repeated path counts once");
        assert!(files.iter().all(|f| f.path().is_absolute()));
        assert!(matches!(files[0], LaunchFile::Base(_)));
        assert!(matches!(files[1], LaunchFile::Epd(_)));
        assert!(matches!(files[2], LaunchFile::Book(_)));
    }

    #[test]
    fn at_most_eight_files_are_kept() {
        let many: Vec<PathBuf> = (0..30).map(|n| PathBuf::from(format!("{n}.pgn"))).collect();
        assert_eq!(launch_files(&many).len(), MAX_LAUNCH_FILES);
    }

    #[test]
    fn the_start_info_carries_the_layout_and_the_options() {
        let cli = parse(&["--no-tip", "x.pgn"]).expect("parse");
        let layout = UserLayout {
            root: Some(PathBuf::from("home").join("MyChess")),
            mode: mychess_app::start::LayoutMode::Normal,
            dirs: None,
            warnings: Vec::new(),
        };
        let info = start_info(&cli, &layout);
        assert_eq!(info.root, layout.root);
        assert!(info.skip_tip);
        assert_eq!(info.files.len(), 1);
    }

    #[test]
    fn portable_mode_writes_nothing_outside_its_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe_dir = tmp.path().join("app dir");
        std::fs::create_dir(&exe_dir).expect("mkdir");
        let inputs = mychess_app::start::LayoutInputs {
            exe_dir: Some(exe_dir.clone()),
            home_dir: Some(tmp.path().join("home")),
            force_portable: true,
            ..Default::default()
        };
        let layout = resolve_layout(&inputs, is_writable_dir);
        let root = layout.root.expect("root");
        assert!(is_inside(&root, &exe_dir));
        let dirs = layout.dirs.expect("dirs");
        assert!(is_inside(&dirs.config, &root) && is_inside(&dirs.data, &root));
        assert!(!tmp.path().join("home").exists(), "the home is untouched");
    }
}
