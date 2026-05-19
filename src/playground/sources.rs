#[cfg(not(target_family = "wasm"))]
use std::io;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use arcstr::literal;
use par_core::source::FileName;
use par_core::workspace::LoadedPackageFile;
#[cfg(not(target_family = "wasm"))]
use par_core::workspace::{
    PackageLayout, SourceOverrides, WorkspaceDiscoveryError, load_package_source_files,
};
use par_runtime::pkgid::PackageId;

use super::examples::PLAYGROUND_EXAMPLES;

pub(super) struct SourceSet {
    kind: SourceSetKind,
    buffers: Vec<SourceBuffer>,
    active: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceSetKind {
    BundledExamples,
    #[cfg(not(target_family = "wasm"))]
    DiskPackage,
}

pub(super) struct SourceBuffer {
    file_name: FileName,
    relative_path_from_src: PathBuf,
    disk_path: Option<PathBuf>,
    source: String,
    saved_source: String,
    reload_mtime: Option<SystemTime>,
}

impl SourceSet {
    pub(super) fn bundled_examples() -> Self {
        let buffers = PLAYGROUND_EXAMPLES
            .iter()
            .map(|example| {
                SourceBuffer::memory(
                    FileName::from(format!(
                        "playground-examples/src/{}",
                        example.relative_path_from_src
                    )),
                    PathBuf::from(example.relative_path_from_src),
                    example.source,
                )
            })
            .collect();

        Self {
            kind: SourceSetKind::BundledExamples,
            buffers,
            active: 0,
        }
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn open_disk(file_path: PathBuf) -> Result<Self, String> {
        let file_path = fs::canonicalize(&file_path).unwrap_or(file_path);
        match PackageLayout::find_from(&file_path) {
            Ok(layout) => Self::from_package_layout(layout, &file_path),
            Err(WorkspaceDiscoveryError::PackageRootNotFound { .. }) => Err(String::from(
                "This file is not a part of a Par package. Create a package using `par new`.",
            )),
            Err(error) => Err(error.to_string()),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn from_package_layout(layout: PackageLayout, active_file: &Path) -> Result<Self, String> {
        let files = load_package_source_files(&layout).map_err(|error| error.to_string())?;
        if files.is_empty() {
            return Err(format!(
                "Package source directory contains no Par files: {}",
                layout.src_dir.display()
            ));
        }

        let buffers = files
            .into_iter()
            .map(SourceBuffer::from_loaded_disk_file)
            .collect::<Vec<_>>();
        let active = buffers
            .iter()
            .position(|buffer| {
                buffer
                    .disk_path
                    .as_deref()
                    .is_some_and(|path| same_path(path, active_file))
            })
            .ok_or_else(|| {
                format!(
                    "File is not a Par source file in its package: {}",
                    active_file.display()
                )
            })?;

        Ok(Self {
            kind: SourceSetKind::DiskPackage,
            buffers,
            active,
        })
    }

    pub(super) fn kind(&self) -> SourceSetKind {
        self.kind
    }

    pub(super) fn active_file_name(&self) -> FileName {
        self.active_buffer().file_name.clone()
    }

    pub(super) fn active_label(&self) -> String {
        let mut label = self.active_buffer().label();
        if self.active_buffer().is_dirty() {
            label.push_str(" *");
        }
        label
    }

    pub(super) fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub(super) fn buffer_label(&self, index: usize) -> String {
        let buffer = &self.buffers[index];
        let mut label = buffer.label();
        if buffer.is_dirty() {
            label.push_str(" *");
        }
        label
    }

    pub(super) fn is_active(&self, index: usize) -> bool {
        self.active == index
    }

    pub(super) fn set_active(&mut self, index: usize) -> bool {
        if index >= self.buffers.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    pub(super) fn active_source(&self) -> &str {
        &self.active_buffer().source
    }

    pub(super) fn active_source_mut(&mut self) -> &mut String {
        &mut self.active_buffer_mut().source
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn can_save_active(&self) -> bool {
        self.active_buffer().disk_path.is_some()
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn save_active(&mut self) -> io::Result<()> {
        let buffer = self.active_buffer_mut();
        let Some(path) = buffer.disk_path.clone() else {
            return Ok(());
        };
        fs::write(&path, buffer.source.as_bytes())?;
        buffer.saved_source = buffer.source.clone();
        buffer.reload_mtime = file_mtime(&path);
        Ok(())
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn active_reload_enabled(&self) -> bool {
        self.active_buffer().reload_mtime.is_some()
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn set_active_reload_enabled(&mut self, enabled: bool) {
        let buffer = self.active_buffer_mut();
        if !enabled {
            buffer.reload_mtime = None;
            return;
        }
        buffer.reload_mtime = buffer.disk_path.as_deref().and_then(file_mtime);
    }

    pub(super) fn reload_active_if_changed(&mut self) {
        let buffer = self.active_buffer_mut();
        let Some(old_mtime) = buffer.reload_mtime else {
            return;
        };
        let Some(path) = buffer.disk_path.as_deref() else {
            return;
        };
        let Some(mtime) = file_mtime(path) else {
            return;
        };
        if !matches!(
            mtime
                .duration_since(old_mtime)
                .map(|x| x > Duration::new(0, 0)),
            Ok(true)
        ) {
            return;
        }
        let Ok(source) = fs::read_to_string(path) else {
            return;
        };
        buffer.source = source.clone();
        buffer.saved_source = source;
        buffer.reload_mtime = Some(mtime);
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn source_overrides(&self) -> SourceOverrides {
        self.buffers
            .iter()
            .filter_map(|buffer| Some((buffer.disk_path.clone()?, buffer.source.clone())))
            .collect()
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) fn active_disk_path(&self) -> Option<&Path> {
        self.active_buffer().disk_path.as_deref()
    }

    pub(super) fn loaded_files(&self) -> Vec<LoadedPackageFile> {
        self.buffers
            .iter()
            .map(|buffer| LoadedPackageFile {
                name: buffer.file_name.clone(),
                relative_path_from_src: buffer.relative_path_from_src.clone(),
                source: buffer.source.clone(),
            })
            .collect()
    }

    pub(super) fn bundled_package_id() -> PackageId {
        PackageId::Special(literal!("playground_examples"))
    }

    fn active_buffer(&self) -> &SourceBuffer {
        &self.buffers[self.active]
    }

    fn active_buffer_mut(&mut self) -> &mut SourceBuffer {
        &mut self.buffers[self.active]
    }
}

impl SourceBuffer {
    fn memory(file_name: FileName, relative_path_from_src: PathBuf, source: &str) -> Self {
        Self {
            file_name,
            relative_path_from_src,
            disk_path: None,
            source: source.to_owned(),
            saved_source: source.to_owned(),
            reload_mtime: None,
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn disk(
        file_name: FileName,
        relative_path_from_src: PathBuf,
        disk_path: PathBuf,
        source: String,
    ) -> Self {
        Self {
            file_name,
            relative_path_from_src,
            reload_mtime: None,
            saved_source: source.clone(),
            source,
            disk_path: Some(disk_path),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn from_loaded_disk_file(file: LoadedPackageFile) -> Self {
        let disk_path = PathBuf::from(file.name.0.as_str());
        Self::disk(
            file.name,
            file.relative_path_from_src,
            disk_path,
            file.source,
        )
    }

    fn label(&self) -> String {
        self.relative_path_from_src
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn is_dirty(&self) -> bool {
        self.source != self.saved_source
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

#[cfg(not(target_family = "wasm"))]
fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path(left) == normalized_path(right)
}

#[cfg(not(target_family = "wasm"))]
fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
