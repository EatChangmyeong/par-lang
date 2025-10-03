use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{
    icombs::readback::Handle,
    par::{
        primitive::ParString,
        process,
        program::{Definition, Module},
        types::Type,
    },
};
use arcstr::literal;
use bytes::Bytes;
use futures::future::BoxFuture;
use tokio::{
    fs::{self, DirEntry, File, OpenOptions, ReadDir},
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};

pub fn external_module() -> Module<std::sync::Arc<process::Expression<()>>> {
    Module {
        type_defs: vec![],
        declarations: vec![],
        definitions: vec![
            Definition::external(
                "Path",
                Type::function(Type::bytes(), Type::name(None, "Path", vec![])),
                |handle| Box::pin(path_from_bytes(handle)),
            ),
            Definition::external("Stdin", Type::name(None, "Reader", vec![]), |handle| {
                Box::pin(stdin_reader(handle))
            }),
            Definition::external("Stdout", Type::name(None, "Writer", vec![]), |handle| {
                Box::pin(stdout_writer(handle))
            }),
        ],
    }
}

async fn path_from_bytes(mut handle: Handle) {
    let b = handle.receive().bytes().await;
    // Unsafe: we accept arbitrary OS-encoded bytes without validation
    let os: &OsStr = unsafe { OsStr::from_encoded_bytes_unchecked(b.as_ref()) };
    let p = PathBuf::from(os);
    provide_path(handle, p);
}

pub fn provide_path(handle: Handle, path: PathBuf) {
    handle.provide_box(move |mut handle| {
        let path = path.clone();
        async move {
            match handle.case().await.as_str() {
                "name" => {
                    let bytes = path
                        .file_name()
                        .map(|n| os_to_bytes(n))
                        .unwrap_or_else(|| Bytes::new());
                    handle.provide_bytes(bytes);
                }
                "absolute" => {
                    let abs = absolute_path(&path);
                    let bytes = os_to_bytes(abs.as_os_str());
                    handle.provide_bytes(bytes);
                }
                "parts" => {
                    provide_bytes_parts(handle, &path);
                }
                "parent" => match path.parent() {
                    Some(p) => {
                        handle.signal(arcstr::literal!("ok"));
                        provide_path(handle, p.to_path_buf());
                    }
                    None => {
                        handle.signal(arcstr::literal!("err"));
                        handle.break_();
                    }
                },
                "append" => {
                    let b = handle.receive().bytes().await;
                    let os: &OsStr = unsafe { OsStr::from_encoded_bytes_unchecked(b.as_ref()) };
                    let p2 = path.join(Path::new(os));
                    provide_path(handle, p2);
                }
                "openFile" => match File::open(&path).await {
                    Ok(file) => {
                        handle.signal(literal!("ok"));
                        return provide_bytes_reader_from_async(handle, file).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "createOrReplaceFile" => match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&path)
                    .await
                {
                    Ok(file) => {
                        handle.signal(literal!("ok"));
                        return provide_bytes_writer_from_async(handle, file).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "createNewFile" => match OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)
                    .await
                {
                    Ok(file) => {
                        handle.signal(literal!("ok"));
                        return provide_bytes_writer_from_async(handle, file).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "appendToFile" => match OpenOptions::new()
                    .write(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    Ok(file) => {
                        handle.signal(literal!("ok"));
                        return provide_bytes_writer_from_async(handle, file).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "createOrAppendToFile" => match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    Ok(file) => {
                        handle.signal(literal!("ok"));
                        return provide_bytes_writer_from_async(handle, file).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "listDir" => match fs::read_dir(&path).await {
                    Ok(mut rd) => {
                        handle.signal(literal!("ok"));
                        return provide_list_dir(handle, &path, &mut rd).await;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                "traverseDir" => match build_dir_tree(path.clone()).await {
                    Ok(nodes) => {
                        handle.signal(literal!("ok"));
                        return provide_dir_tree(handle, nodes.as_slice());
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err));
                    }
                },
                "createDir" => match fs::create_dir_all(&path).await {
                    Ok(()) => {
                        handle.signal(literal!("ok"));
                        return handle.break_();
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                },
                _ => unreachable!(),
            }
        }
    })
}

fn absolute_path(p: &Path) -> PathBuf {
    match p.canonicalize() {
        Ok(abs) => abs,
        Err(_) => {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(p)
            }
        }
    }
}

fn provide_bytes_parts(mut handle: Handle, p: &Path) {
    for part in p.iter() {
        handle.signal(arcstr::literal!("item"));
        let bytes = os_to_bytes(part);
        handle.send().provide_bytes(bytes);
    }
    handle.signal(arcstr::literal!("end"));
    handle.break_();
}

#[cfg(unix)]
fn os_to_bytes(os: &OsStr) -> Bytes {
    use std::os::unix::ffi::OsStrExt;
    Bytes::copy_from_slice(os.as_bytes())
}

#[cfg(windows)]
fn os_to_bytes(os: &OsStr) -> Bytes {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = os.encode_wide().collect();
    let mut bytes = Vec::with_capacity(wide.len() * 2);
    for w in wide {
        bytes.push((w & 0xFF) as u8);
        bytes.push((w >> 8) as u8);
    }
    Bytes::from(bytes)
}

#[cfg(not(any(unix, windows)))]
fn os_to_bytes(os: &OsStr) -> Bytes {
    Bytes::from(os.to_string_lossy().as_ref())
}

async fn provide_bytes_reader_from_async(mut handle: Handle, mut reader: impl AsyncRead + Unpin) {
    let mut buf = vec![0u8; 512];
    loop {
        match handle.case().await.as_str() {
            "close" => {
                handle.signal(literal!("ok"));
                return handle.break_();
            }
            "read" => match reader.read(&mut buf[..]).await {
                Ok(n) => {
                    if n == 0 {
                        handle.signal(literal!("ok"));
                        handle.signal(literal!("end"));
                        return handle.break_();
                    }
                    handle.signal(literal!("ok"));
                    handle.signal(literal!("chunk"));
                    handle
                        .send()
                        .provide_bytes(Bytes::copy_from_slice(&buf[..n]));
                    continue;
                }
                Err(err) => {
                    handle.signal(literal!("err"));
                    return handle.provide_string(ParString::from(err.to_string()));
                }
            },
            _ => unreachable!(),
        }
    }
}

async fn provide_bytes_writer_from_async(mut handle: Handle, mut writer: impl AsyncWrite + Unpin) {
    loop {
        match handle.case().await.as_str() {
            "close" => {
                // Try to flush pending data before closing
                match writer.flush().await {
                    Ok(()) => {
                        handle.signal(literal!("ok"));
                        return handle.break_();
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                }
            }
            "flush" => match writer.flush().await {
                Ok(()) => {
                    handle.signal(literal!("ok"));
                    continue;
                }
                Err(err) => {
                    handle.signal(literal!("err"));
                    return handle.provide_string(ParString::from(err.to_string()));
                }
            },
            "write" => {
                let bytes = handle.receive().bytes().await;
                match writer.write_all(bytes.as_ref()).await {
                    Ok(()) => {
                        handle.signal(literal!("ok"));
                        continue;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}

// Provide List<self/append> for the directory entries of `base` using a pre-opened ReadDir.
async fn provide_list_dir(mut handle: Handle, base: &Path, rd: &mut ReadDir) {
    let mut entries: Vec<(Bytes, std::ffi::OsString)> = Vec::new();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name();
        // Sort key: raw bytes if available, fallback to lossy string
        let key = os_to_bytes(&name);
        entries.push((key, name));
    }
    // Sort deterministically by the byte-representation of file name
    entries.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

    for (_, name) in entries {
        let child = base.join(Path::new(&name));
        handle.signal(literal!("item"));
        provide_path(handle.send(), child);
    }
    handle.signal(literal!("end"));
    handle.break_();
}

// Directory tree node used for traverseDir
enum DirNode {
    File(PathBuf),
    Dir {
        path: PathBuf,
        children: Vec<DirNode>,
    },
}

// Recursively build the full directory tree. Returns an error message if any IO fails.
fn build_dir_tree(dir: PathBuf) -> BoxFuture<'static, Result<Vec<DirNode>, String>> {
    Box::pin(async move {
        let mut rd = fs::read_dir(&dir).await.map_err(|e| format!("{}", e))?;

        // Collect entries first to allow deterministic sorting
        let mut items: Vec<(Bytes, DirEntry)> = Vec::new();
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name();
            let key = os_to_bytes(&name);
            items.push((key, entry));
        }
        items.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

        let mut result = Vec::new();
        for (_, entry) in items {
            let ty = entry.file_type().await.map_err(|e| format!("{}", e))?;
            let child_path = entry.path();
            if ty.is_dir() {
                let children = build_dir_tree(child_path.clone()).await?;
                result.push(DirNode::Dir {
                    path: child_path,
                    children,
                });
            } else {
                // Treat symlinks and others as files to avoid cycles
                result.push(DirNode::File(child_path));
            }
        }
        Ok(result)
    })
}

fn provide_dir_tree(mut handle: Handle, nodes: &[DirNode]) {
    match nodes.split_first() {
        None => {
            handle.signal(literal!("end"));
            handle.break_();
        }
        Some((node, tail)) => match node {
            DirNode::File(path) => {
                handle.signal(literal!("file"));
                provide_path(handle.send(), path.clone());
                provide_dir_tree(handle, tail);
            }
            DirNode::Dir { path, children } => {
                handle.signal(literal!("dir"));
                provide_path(handle.send(), path.clone());
                provide_dir_tree(handle.send(), children.as_slice());
                provide_dir_tree(handle, tail);
            }
        },
    }
}

async fn stdin_reader(handle: Handle) {
    provide_bytes_reader_from_async(handle, tokio::io::stdin()).await;
}

async fn stdout_writer(handle: Handle) {
    provide_bytes_writer_from_async(handle, tokio::io::stdout()).await;
}
