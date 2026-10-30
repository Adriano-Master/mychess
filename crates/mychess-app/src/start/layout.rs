//! Where the `MyChess` folder of the user is (D4000, D4013).
//!
//! Precedence, highest first: `--portable` on the command line, then
//! `$MYCHESS_HOME` (the folder `$MYCHESS_HOME/MyChess`), then a
//! `portable.flag` next to the executable, then the home folder
//! (`~/MyChess`). The program never becomes portable by itself.

use std::path::{Path, PathBuf};

use directories::UserDirs;

/// Name of the visible folder (never translated).
pub const ROOT_FOLDER: &str = "MyChess";
/// The file next to the executable that turns the portable mode on.
pub const PORTABLE_FLAG: &str = "portable.flag";
/// The environment variable that replaces the home folder.
pub const HOME_ENV: &str = "MYCHESS_HOME";

/// The facts the choice depends on (injected, so tests need no real home).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutInputs {
    /// The folder of the executable.
    pub exe_dir: Option<PathBuf>,
    /// The home folder of the user.
    pub home_dir: Option<PathBuf>,
    /// `$MYCHESS_HOME`, already absolute; `None` when unset or empty.
    pub env_home: Option<PathBuf>,
    /// `--portable` was given.
    pub force_portable: bool,
    /// `portable.flag` exists next to the executable.
    pub flag_present: bool,
}

/// Which rule chose the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// The home folder of the user.
    Normal,
    /// Next to the executable.
    Portable,
    /// `$MYCHESS_HOME`.
    EnvHome,
}

/// Something the user should know about the choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutWarning {
    /// The portable folder cannot be written: the program went back to the
    /// normal mode.
    PortableNotWritable(PathBuf),
    /// The portable mode was asked for but the executable's folder is
    /// unknown.
    PortableUnavailable,
    /// There is no home folder and nothing else to use.
    NoHomeFolder,
}

/// The result of [`resolve_layout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLayout {
    /// The `MyChess` folder, `None` when there is no place for it.
    pub root: Option<PathBuf>,
    /// Which rule chose it.
    pub mode: LayoutMode,
    /// The directories that replace the ones of the system (portable mode
    /// only).
    pub dirs: Option<super::AppDirs>,
    /// What went wrong on the way.
    pub warnings: Vec<LayoutWarning>,
}

/// Chooses the folder. `writable` says whether a directory can be written
/// (it may create it); it is asked only for the portable folder.
pub fn resolve_layout(inputs: &LayoutInputs, writable: impl Fn(&Path) -> bool) -> UserLayout {
    let mut warnings = Vec::new();
    // `--portable` beats the variable; the flag file does not.
    let try_portable = inputs.force_portable || (inputs.env_home.is_none() && inputs.flag_present);
    if try_portable {
        match inputs.exe_dir.as_deref() {
            Some(exe) => {
                let root = exe.join(ROOT_FOLDER);
                if writable(&root) {
                    let dirs = super::AppDirs {
                        config: root.join(".config"),
                        data: root.join(".data"),
                    };
                    return UserLayout {
                        root: Some(root),
                        mode: LayoutMode::Portable,
                        dirs: Some(dirs),
                        warnings,
                    };
                }
                warnings.push(LayoutWarning::PortableNotWritable(root));
            }
            None => warnings.push(LayoutWarning::PortableUnavailable),
        }
    }
    if let Some(env) = &inputs.env_home {
        return UserLayout {
            root: Some(env.join(ROOT_FOLDER)),
            mode: LayoutMode::EnvHome,
            dirs: None,
            warnings,
        };
    }
    let root = inputs.home_dir.as_ref().map(|home| home.join(ROOT_FOLDER));
    if root.is_none() {
        warnings.push(LayoutWarning::NoHomeFolder);
    }
    UserLayout {
        root,
        mode: LayoutMode::Normal,
        dirs: None,
        warnings,
    }
}

/// Reads the real environment: the executable, the home folder, the
/// variable and the flag file. A few `stat` calls; run it before the window
/// exists.
pub fn detect_inputs(force_portable: bool) -> LayoutInputs {
    // The real file, so a symbolic link to the executable finds the flag
    // next to the program and not next to the link.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::canonicalize(&exe).ok().or(Some(exe)))
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let env_home = std::env::var_os(HOME_ENV)
        .filter(|value| !value.is_empty())
        .and_then(|value| std::path::absolute(PathBuf::from(value)).ok());
    LayoutInputs {
        flag_present: exe_dir
            .as_deref()
            .is_some_and(|dir| dir.join(PORTABLE_FLAG).is_file()),
        exe_dir,
        home_dir: UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf()),
        env_home,
        force_portable,
    }
}

/// Whether files can be created in `dir` (created if missing). Leaves
/// nothing behind: the probe is an anonymous temporary file.
pub fn is_writable_dir(dir: &Path) -> bool {
    std::fs::create_dir_all(dir).is_ok() && tempfile::tempfile_in(dir).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> LayoutInputs {
        LayoutInputs {
            exe_dir: Some(PathBuf::from("exe")),
            home_dir: Some(PathBuf::from("home")),
            ..LayoutInputs::default()
        }
    }

    #[test]
    fn normal_mode_uses_the_home_folder() {
        let layout = resolve_layout(&inputs(), |_| true);
        assert_eq!(layout.root, Some(PathBuf::from("home").join("MyChess")));
        assert_eq!(layout.mode, LayoutMode::Normal);
        assert!(layout.dirs.is_none() && layout.warnings.is_empty());
    }

    #[test]
    fn the_flag_file_makes_the_program_portable() {
        let layout = resolve_layout(
            &LayoutInputs {
                flag_present: true,
                ..inputs()
            },
            |_| true,
        );
        let root = PathBuf::from("exe").join("MyChess");
        assert_eq!(layout.mode, LayoutMode::Portable);
        assert_eq!(layout.root, Some(root.clone()));
        let dirs = layout.dirs.expect("portable dirs");
        assert_eq!(dirs.config, root.join(".config"));
        assert_eq!(dirs.data, root.join(".data"));
    }

    #[test]
    fn without_the_flag_nothing_is_portable() {
        let layout = resolve_layout(&inputs(), |_| panic!("not asked outside portable mode"));
        assert_eq!(layout.mode, LayoutMode::Normal);
    }

    #[test]
    fn an_unwritable_portable_folder_falls_back_with_a_warning() {
        let layout = resolve_layout(
            &LayoutInputs {
                flag_present: true,
                ..inputs()
            },
            |_| false,
        );
        assert_eq!(layout.mode, LayoutMode::Normal);
        assert_eq!(layout.root, Some(PathBuf::from("home").join("MyChess")));
        assert!(layout.dirs.is_none());
        assert_eq!(
            layout.warnings,
            vec![LayoutWarning::PortableNotWritable(
                PathBuf::from("exe").join("MyChess")
            )]
        );
    }

    #[test]
    fn the_variable_beats_the_flag_file_but_not_the_option() {
        let with_env = LayoutInputs {
            env_home: Some(PathBuf::from("elsewhere")),
            flag_present: true,
            ..inputs()
        };
        let layout = resolve_layout(&with_env, |_| true);
        assert_eq!(layout.mode, LayoutMode::EnvHome);
        assert_eq!(
            layout.root,
            Some(PathBuf::from("elsewhere").join("MyChess"))
        );
        let forced = LayoutInputs {
            force_portable: true,
            ..with_env
        };
        assert_eq!(resolve_layout(&forced, |_| true).mode, LayoutMode::Portable);
    }

    #[test]
    fn portable_without_a_known_executable_folder_warns() {
        let layout = resolve_layout(
            &LayoutInputs {
                exe_dir: None,
                force_portable: true,
                ..inputs()
            },
            |_| true,
        );
        assert_eq!(layout.mode, LayoutMode::Normal);
        assert_eq!(layout.warnings, vec![LayoutWarning::PortableUnavailable]);
    }

    #[test]
    fn no_home_folder_gives_no_root_and_a_warning() {
        let layout = resolve_layout(&LayoutInputs::default(), |_| true);
        assert_eq!(layout.root, None);
        assert_eq!(layout.warnings, vec![LayoutWarning::NoHomeFolder]);
    }

    #[test]
    fn paths_with_spaces_and_accents_are_kept() {
        let home = PathBuf::from("Usuário com espaço");
        let layout = resolve_layout(
            &LayoutInputs {
                home_dir: Some(home.clone()),
                ..LayoutInputs::default()
            },
            |_| true,
        );
        assert_eq!(layout.root, Some(home.join("MyChess")));
    }

    #[test]
    fn writable_probe_creates_the_folder_and_leaves_it_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("a b").join("MyChess");
        assert!(is_writable_dir(&dir));
        assert_eq!(std::fs::read_dir(&dir).expect("read").count(), 0);
    }

    #[test]
    fn a_file_in_the_way_is_not_writable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("MyChess");
        std::fs::write(&file, b"x").expect("write");
        assert!(!is_writable_dir(&file));
    }
}
