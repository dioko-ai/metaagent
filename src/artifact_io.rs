use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{fs::Permissions, os::unix::fs::PermissionsExt};

use crate::default_config::DEFAULT_CONFIG_TOML;

pub fn read_text_file(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    Ok(text)
}

pub fn write_text_file(path: &Path, text: &str) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(text.as_bytes())?;
    writer.flush()
}

pub fn write_text_file_if_missing(path: &Path, text: &str) -> io::Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => {
            let mut writer = BufWriter::new(file);
            writer.write_all(text.as_bytes())?;
            writer.flush()?;
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

pub fn home_dir() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

pub fn metaagent_config_file_path() -> io::Result<PathBuf> {
    let config_dir = home_dir()?.join(".metaagent");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("config.toml"))
}

pub fn ensure_default_metaagent_config() -> io::Result<PathBuf> {
    let config_file = metaagent_config_file_path()?;
    let existing_text = match read_text_file(&config_file) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let merged_text = merge_default_config_with_user_overrides(existing_text.as_deref())?;
    if existing_text.as_deref() != Some(merged_text.as_str()) {
        write_text_file_atomic(&config_file, &merged_text)?;
    }
    Ok(config_file)
}

pub fn load_merged_metaagent_config_text() -> io::Result<String> {
    let config_file = ensure_default_metaagent_config()?;
    read_text_file(&config_file)
}

#[cfg(test)]
pub(crate) fn home_env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn merge_default_config_with_user_overrides(override_text: Option<&str>) -> io::Result<String> {
    let mut merged = parse_toml_table(DEFAULT_CONFIG_TOML)?;
    let override_value = parse_toml_table(override_text.unwrap_or_default())?;
    merge_toml_tables(&mut merged, override_value);
    toml::to_string_pretty(&merged).map_err(io::Error::other)
}

fn parse_toml_table(text: &str) -> io::Result<toml::Value> {
    if text.trim().is_empty() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }
    toml::from_str(text).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn merge_toml_tables(base: &mut toml::Value, override_value: toml::Value) {
    match (base, override_value) {
        (toml::Value::Table(base_map), toml::Value::Table(override_map)) => {
            for (key, override_item) in override_map {
                if let Some(base_item) = base_map.get_mut(&key) {
                    merge_toml_tables(base_item, override_item);
                } else {
                    base_map.insert(key, override_item);
                }
            }
        }
        (base_slot, override_item) => {
            *base_slot = override_item;
        }
    }
}

fn write_text_file_atomic(path: &Path, text: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "target path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config.toml");
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..16u8 {
        let tmp = parent.join(format!(".{file_name}.tmp-{pid}-{nanos}-{attempt}"));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => {
                ensure_owner_only_permissions(&tmp)?;
                let mut writer = BufWriter::new(file);
                writer.write_all(text.as_bytes())?;
                writer.flush()?;
                writer.get_ref().sync_all()?;
                if let Err(err) = fs::rename(&tmp, path) {
                    let _ = fs::remove_file(&tmp);
                    return Err(err);
                }
                sync_directory(parent)?;
                return Ok(());
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to allocate temporary config file name",
    ))
}

#[cfg(unix)]
fn ensure_owner_only_permissions(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn ensure_owner_only_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
