//! 独立客户端本地附件：仅从用户选择或文件剪贴板取得路径，不接受网页传入任意本机路径。
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Serialize;

// 与网页 local-inputs.js、服务端 input_files.rs 同步：16 个顶层附件、512 个目录项、合计 12 MiB。
const MAX_ROOTS: usize = 16;
const MAX_ENTRIES: usize = 512;
const MAX_BYTES: u64 = 12 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct LocalInput {
    kind: &'static str,
    name: String,
    size: u64,
    files: Vec<LocalFile>,
    directories: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LocalFile {
    path: String,
    data: String,
}

fn regular_metadata(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("读取附件失败：{error}"))?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.is_symlink() || reparse || !metadata.is_file() && !metadata.is_dir() {
        return Err("附件包含符号链接、重解析点或特殊文件，请选择普通文件和文件夹。".into());
    }
    Ok(metadata)
}

pub(crate) fn read(paths: Vec<PathBuf>) -> Result<Vec<LocalInput>, String> {
    if paths.len() > MAX_ROOTS {
        return Err(format!("一次最多选择 {MAX_ROOTS} 个文件或文件夹。"));
    }
    let mut total = 0u64;
    let mut count = paths.len();
    let mut result = Vec::new();
    for path in paths {
        let metadata = regular_metadata(&path)?;
        let selected_root = std::fs::canonicalize(&path).map_err(|_| "无法确认附件的实际位置。")?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|name| !name.is_empty() && name.len() <= 255)
            .ok_or("附件名称无效或过长。")?
            .to_owned();
        let directory = metadata.is_dir();
        let mut input = LocalInput {
            kind: if directory { "folder" } else { "file" },
            name: name.clone(),
            size: 0,
            files: Vec::new(),
            directories: Vec::new(),
        };
        let mut pending = vec![(
            path.clone(),
            if directory { String::new() } else { name },
            0usize,
        )];
        while let Some((entry, relative, depth)) = pending.pop() {
            if !relative.is_empty() {
                count += 1;
            }
            if count > MAX_ENTRIES || depth > 32 {
                return Err("文件夹最多包含 512 个目录项，目录层级不能超过 32 层。".into());
            }
            let metadata = regular_metadata(&entry)?;
            if metadata.is_dir() {
                if !relative.is_empty() {
                    input.directories.push(relative.clone());
                }
                let children = std::fs::read_dir(&entry)
                    .map_err(|error| format!("无法读取文件夹：{error}"))?;
                for child in children {
                    let child = child.map_err(|error| format!("无法读取目录项：{error}"))?;
                    let name = child
                        .file_name()
                        .into_string()
                        .map_err(|_| "文件名称编码无效。")?;
                    let child_relative = if relative.is_empty() {
                        name
                    } else {
                        format!("{relative}/{name}")
                    };
                    pending.push((child.path(), child_relative, depth + 1));
                    if count + pending.len() > MAX_ENTRIES {
                        return Err("文件夹最多包含 512 个目录项。".into());
                    }
                }
            } else {
                if total.saturating_add(metadata.len()) > MAX_BYTES {
                    return Err("文件和文件夹的总大小不能超过 12 MiB。".into());
                }
                // 先按元数据限制大小，再有限读取；复制/选取期间文件增长不能突破上传内存上限。
                use std::io::Read;
                let mut bytes = Vec::new();
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT：不跟随最终项被替换的链接。
                }
                let file = options
                    .open(&entry)
                    .map_err(|error| format!("无法打开附件：{error}"))?;
                #[cfg(windows)]
                {
                    use std::os::windows::ffi::OsStringExt;
                    use std::os::windows::fs::MetadataExt;
                    use std::os::windows::io::AsRawHandle;
                    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
                    let metadata = file.metadata().map_err(|_| "无法确认附件文件类型。")?;
                    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
                        return Err("附件已被替换为链接或特殊文件，请重新选择。".into());
                    }
                    let mut buffer = vec![0u16; 32_768];
                    let length = unsafe {
                        GetFinalPathNameByHandleW(
                            file.as_raw_handle() as _,
                            buffer.as_mut_ptr(),
                            buffer.len() as u32,
                            0,
                        )
                    } as usize;
                    if length == 0
                        || length >= buffer.len()
                        || !PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]))
                            .starts_with(&selected_root)
                    {
                        return Err("附件的实际位置已超出所选范围，请重新选择。".into());
                    }
                }
                #[cfg(not(windows))]
                if !std::fs::canonicalize(&entry)
                    .map_err(|_| "无法确认附件位置。")?
                    .starts_with(&selected_root)
                {
                    return Err("附件的实际位置已超出所选范围。".into());
                }
                file.take(MAX_BYTES - total + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| format!("无法读取附件：{error}"))?;
                if total + bytes.len() as u64 > MAX_BYTES {
                    return Err("文件和文件夹的总大小不能超过 12 MiB。".into());
                }
                // 再次核对路径，避免读取时被替换为重解析点。
                regular_metadata(&entry)?;
                total += bytes.len() as u64;
                input.size += bytes.len() as u64;
                input.files.push(LocalFile {
                    path: relative,
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
        }
        input
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        input.directories.sort();
        result.push(input);
    }
    Ok(result)
}

#[cfg(windows)]
pub(crate) fn choose(parent: isize, folders: bool) -> Result<Vec<PathBuf>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法打开文件选择器：{error}"))?;
        let mut options = dialog.GetOptions().map_err(|error| error.to_string())?
            | FOS_ALLOWMULTISELECT
            | FOS_FORCEFILESYSTEM
            | FOS_PATHMUSTEXIST
            | FOS_FILEMUSTEXIST;
        if folders {
            options |= FOS_PICKFOLDERS;
        }
        dialog
            .SetOptions(options)
            .map_err(|error| error.to_string())?;
        if let Err(error) = dialog.Show(Some(HWND(parent as _))) {
            if error.code().0 as u32 == 0x800704c7 {
                return Ok(Vec::new());
            }
            return Err(format!("文件选择失败：{error}"));
        }
        let items = dialog.GetResults().map_err(|error| error.to_string())?;
        let count = items.GetCount().map_err(|error| error.to_string())?;
        if count as usize > MAX_ROOTS {
            return Err("一次最多选择 16 个文件或文件夹。".into());
        }
        let mut paths = Vec::new();
        for index in 0..count {
            let item = items.GetItemAt(index).map_err(|error| error.to_string())?;
            let name = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|error| error.to_string())?;
            let path = name.to_string().map_err(|error| error.to_string());
            CoTaskMemFree(Some(name.0 as _));
            paths.push(PathBuf::from(path?));
        }
        Ok(paths)
    }
}

#[cfg(windows)]
pub(crate) fn clipboard() -> Result<Vec<PathBuf>, String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::UI::Shell::DragQueryFileW;
    // 用户要求资源管理器 Ctrl+C / 输入区 Ctrl+V；只读取 CF_HDROP，不把普通文字当作本机路径。
    const CF_HDROP: u32 = 15;
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return Ok(Vec::new());
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("剪贴板正被其他程序占用，请重新粘贴。".into());
        }
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        let data = GetClipboardData(CF_HDROP);
        if data.is_null() {
            return Err("无法读取复制的文件和文件夹。".into());
        }
        let count = DragQueryFileW(data as _, u32::MAX, std::ptr::null_mut(), 0);
        if count as usize > MAX_ROOTS {
            return Err("一次最多粘贴 16 个文件或文件夹。".into());
        }
        let mut paths = Vec::new();
        for index in 0..count {
            let length = DragQueryFileW(data as _, index, std::ptr::null_mut(), 0);
            if length == 0 || length > 32_767 {
                return Err("复制的文件路径无效。".into());
            }
            let mut buffer = vec![0u16; length as usize + 1];
            if DragQueryFileW(data as _, index, buffer.as_mut_ptr(), buffer.len() as u32) != length
            {
                return Err("剪贴板内容已变化，请重新复制。".into());
            }
            use std::os::windows::ffi::OsStringExt;
            paths.push(PathBuf::from(std::ffi::OsString::from_wide(
                &buffer[..length as usize],
            )));
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_inputs_read_files_nested_folders_and_empty_directories() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("素材");
        std::fs::create_dir_all(directory.join("子目录/空目录")).unwrap();
        std::fs::write(directory.join("子目录/说明.txt"), b"hello").unwrap();
        let file = temporary.path().join("empty.txt");
        std::fs::write(&file, []).unwrap();
        let inputs = read(vec![directory, file]).unwrap();
        assert_eq!(inputs[0].kind, "folder");
        assert_eq!(inputs[0].size, 5);
        assert_eq!(inputs[0].directories, ["子目录", "子目录/空目录"]);
        assert_eq!(inputs[0].files[0].path, "子目录/说明.txt");
        assert_eq!(inputs[0].files[0].data, "aGVsbG8=");
        assert_eq!(inputs[1].kind, "file");
        assert_eq!(inputs[1].size, 0);
        assert_eq!(inputs[1].files[0].data, "");
    }

    #[test]
    fn local_inputs_reject_missing_files_and_limits_without_skipping_entries() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(read(vec![temporary.path().join("missing")]).is_err());
        assert!(
            read(vec![temporary.path().to_owned(); 17])
                .unwrap_err()
                .contains("16")
        );
        let file = temporary.path().join("large.bin");
        std::fs::File::create(&file)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(read(vec![file]).unwrap_err().contains("12 MiB"));
    }
}
