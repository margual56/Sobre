use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

const ICON: &[u8] = include_bytes!("../icons/128x128@2x.png");

/// Where an installed copy lives, following the XDG conventions.
pub struct Places {
    pub program: PathBuf,
    pub desktop_entry: PathBuf,
    pub icon: PathBuf,
}

impl Places {
    pub fn under(home: &Path, data_home: &Path) -> Self {
        Self {
            program: home.join(".local/bin/Sobre.AppImage"),
            desktop_entry: data_home.join("applications/sobre.desktop"),
            icon: data_home.join("icons/hicolor/256x256/apps/sobre.png"),
        }
    }

    pub fn for_user() -> Result<Self> {
        let home =
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?);
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        Ok(Self::under(&home, &data_home))
    }

    pub fn is_installed(&self) -> bool {
        self.desktop_entry.is_file() && self.program.is_file()
    }
}

/// Quote a path for the Exec line of a desktop entry.
fn exec_quote(path: &Path) -> String {
    let mut out = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    // The entry format reads the value once more before the shell-style quoting.
    out.replace('\\', "\\\\").replace('%', "%%")
}

pub fn desktop_entry(program: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Sobre\nGenericName=Email Client\nComment=A small local email client\n\
         Exec={}\nTryExec={}\nIcon=sobre\nTerminal=false\nCategories=Network;Email;\nKeywords=mail;email;inbox;\n\
         StartupWMClass=sobre\nStartupNotify=true\n",
        exec_quote(program),
        program.display()
    )
}

fn write_atomically(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent folder", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let staged = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("sobre")
    ));
    std::fs::write(&staged, bytes)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&staged, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Copy the AppImage at `source` into place and register it with the desktop.
pub fn install(source: &Path, places: &Places) -> Result<()> {
    if source != places.program {
        let dir = places
            .program
            .parent()
            .ok_or_else(|| anyhow!("no folder for the program"))?;
        std::fs::create_dir_all(dir)?;
        let staged = dir.join(".Sobre.AppImage.tmp");
        std::fs::copy(source, &staged).with_context(|| format!("copying to {}", dir.display()))?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&staged, &places.program)?;
    }
    write_atomically(&places.icon, ICON, 0o644)?;
    write_atomically(
        &places.desktop_entry,
        desktop_entry(&places.program).as_bytes(),
        0o644,
    )?;
    Ok(())
}

/// Remove what `install` added. Mail and settings are left alone.
pub fn uninstall(places: &Places) -> Result<()> {
    for path in [&places.desktop_entry, &places.icon, &places.program] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow!(e).context(format!("removing {}", path.display()))),
        }
    }
    Ok(())
}

/// Tell the desktop its menus changed. Each tool is optional.
pub fn refresh_menus(places: &Places) {
    let run = |program: &str, args: &[&std::ffi::OsStr]| {
        std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
    };
    if let Some(dir) = places.desktop_entry.parent() {
        run("update-desktop-database", &[dir.as_os_str()]);
    }
    if let Some(theme) = places.icon.ancestors().nth(3) {
        run(
            "gtk-update-icon-cache",
            &["-q".as_ref(), "-t".as_ref(), theme.as_os_str()],
        );
    }
    run("kbuildsycoca6", &["--noincremental".as_ref()]);
}

#[derive(Serialize)]
pub struct InstallStatus {
    /// Only an AppImage copy can install itself.
    pub available: bool,
    pub installed: bool,
    /// This process is the installed copy.
    pub running_installed: bool,
    pub program: String,
}

pub fn status() -> InstallStatus {
    let running = crate::update::running_appimage();
    match Places::for_user() {
        Ok(places) => InstallStatus {
            available: running.is_some(),
            installed: places.is_installed(),
            running_installed: running.as_deref() == Some(places.program.as_path()),
            program: places.program.display().to_string(),
        },
        Err(_) => InstallStatus {
            available: false,
            installed: false,
            running_installed: false,
            program: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_uninstalls_under_a_home() {
        let home = std::env::temp_dir().join(format!(
            "sobre home $x-{}",
            crate::db::key::to_hex(&crate::db::key::random_bytes::<6>())
        ));
        let downloads = home.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let source = downloads.join("Sobre_0.2.0_amd64.AppImage");
        std::fs::write(&source, b"\x7fELFprogram").unwrap();
        let places = Places::under(&home, &home.join(".local/share"));
        assert!(!places.is_installed());

        install(&source, &places).unwrap();
        assert!(places.is_installed());
        assert_eq!(std::fs::read(&places.program).unwrap(), b"\x7fELFprogram");
        assert_eq!(
            std::fs::metadata(&places.program)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(std::fs::read(&places.icon).unwrap().starts_with(b"\x89PNG"));
        let entry = std::fs::read_to_string(&places.desktop_entry).unwrap();
        assert!(entry.contains("Name=Sobre\n") && entry.contains("Icon=sobre\n"));
        // A space and a dollar sign in the path survive the Exec quoting.
        assert!(entry.contains(&format!(
            "Exec=\"{}\"\n",
            places.program.display().to_string().replace('$', "\\\\$")
        )));
        assert!(source.exists());

        // Installing again from the installed copy is harmless.
        install(&places.program, &places).unwrap();
        assert_eq!(std::fs::read(&places.program).unwrap(), b"\x7fELFprogram");

        uninstall(&places).unwrap();
        assert!(!places.is_installed() && !places.icon.exists() && !places.program.exists());
        uninstall(&places).unwrap();
        std::fs::remove_dir_all(home).ok();
    }
}
