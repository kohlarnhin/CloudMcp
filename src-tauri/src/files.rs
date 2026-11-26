use crate::store::{clipped, err, AppResult, Store};
use cap_std::fs::{Dir, File, OpenOptions, Permissions};
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

const MAX_WRITE_BYTES: usize = 1024 * 1024;
const BINARY_CONTENT: &str = "文件包含二进制内容；图片请使用 fs_image 接口";
const READ_TEXT_LIMIT: usize = 32 * 1024;
const READ_LINES_DEFAULT: usize = 400;
const READ_LINES_MAX: usize = 2000;
const READ_LINE_CHARS: usize = 2000;
// Enough bytes for READ_LINE_CHARS characters of up to 4 bytes, plus one more.
const READ_LINE_KEEP_BYTES: usize = READ_LINE_CHARS * 4 + 4;
const LINE_CLIPPED: &str = "…[行已截断]";
const COUNT_LINES_MAX_BYTES: u64 = 8 * 1024 * 1024;
const INVALID_UTF8: &str = "文件不是有效的 UTF-8 文本；行模式无法用于精确编辑，请改用字节模式读取";
const EDIT_TARGET_REQUIRED: &str = "目标必须是已存在的普通文件，不能是目录或符号链接";

// Keep an edit's read and replacement ordered with other MCP file mutations.
static FILE_MUTATIONS: Mutex<()> = Mutex::new(());

pub fn relative(path: &str, allow_root: bool) -> AppResult<PathBuf> {
    let p = Path::new(path);
    if p.components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err("请使用授权目录内的相对路径，不能包含 .. 或绝对路径".into());
    }
    let p: PathBuf = p
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();
    if p.as_os_str().is_empty() {
        if allow_root {
            return Ok(PathBuf::from("."));
        }
        return Err("不能修改或删除授权目录本身".into());
    }
    Ok(p)
}
pub fn root(store: &Store, scope_id: &str) -> AppResult<Arc<Dir>> {
    let scope = store.scope(scope_id)?;
    store
        .roots
        .lock()
        .unwrap()
        .get(&scope.id)
        .cloned()
        .ok_or_else(|| "授权目录当前不可用，请在本机移除后重新添加".into())
}
pub fn list(store: &Store, scope: &str, path: &str, offset: usize) -> AppResult<Value> {
    if offset > 10000 {
        return Err("目录分页超出上限，请使用更具体的子目录".into());
    }
    let dir = root(store, scope)?;
    let entries = dir.read_dir(relative(path, true)?).map_err(err)?;
    let mut rows = Vec::new();
    for e in entries.skip(offset).take(101) {
        let e = e.map_err(err)?;
        let kind = e.file_type().map_err(err)?;
        rows.push(json!({"name":e.file_name().to_string_lossy(),"kind":if kind.is_dir(){"directory"}else if kind.is_symlink(){"symlink"}else{"file"}, "bytes":e.metadata().ok().map(|m|m.len())}));
    }
    let more = rows.len() > 100;
    rows.truncate(100);
    Ok(
        json!({"entries":rows,"next_offset":if more {Some(offset+100)} else {None},"pagination":"directory enumeration order"}),
    )
}
pub fn read(store: &Store, scope: &str, path: &str, offset: u64, limit: usize) -> AppResult<Value> {
    let dir = root(store, scope)?;
    let mut file = dir.open(relative(path, false)?).map_err(err)?;
    if !file.metadata().map_err(err)?.is_file() {
        return Err("只能读取普通文件".into());
    }
    let total = file.metadata().map_err(err)?.len();
    file.seek(SeekFrom::Start(offset)).map_err(err)?;
    let mut bytes = Vec::new();
    file.take(limit.clamp(1, 32768) as u64)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.contains(&0) {
        return Err(BINARY_CONTENT.into());
    }
    let next = offset + bytes.len() as u64;
    Ok(
        json!({"text":String::from_utf8_lossy(&bytes),"offset":offset,"next_offset":next,"total_bytes":total,"truncated":next<total}),
    )
}
/// Reads whole lines with `cat -n` style numbers so callers can locate text for fs_edit.
pub fn read_lines(
    store: &Store,
    scope: &str,
    path: &str,
    line: u64,
    limit: Option<usize>,
) -> AppResult<Value> {
    if line == 0 {
        return Err("line 从 1 开始".into());
    }
    let limit = limit.unwrap_or(READ_LINES_DEFAULT).clamp(1, READ_LINES_MAX);
    let dir = root(store, scope)?;
    let file = dir.open(relative(path, false)?).map_err(err)?;
    let metadata = file.metadata().map_err(err)?;
    if !metadata.is_file() {
        return Err("只能读取普通文件".into());
    }
    let count_all = metadata.len() <= COUNT_LINES_MAX_BYTES;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut text = String::new();
    let mut number = 0u64;
    let mut taken = 0usize;
    let mut end_line = None;
    let mut next_line = None;
    let mut crlf = false;
    loop {
        let wanted = number + 1 >= line && next_line.is_none() && taken < limit;
        let keep = if wanted { READ_LINE_KEEP_BYTES } else { 0 };
        let Some(scanned) = scan_line(&mut reader, keep, &mut buffer)? else {
            break;
        };
        number += 1;
        if number < line {
            continue;
        }
        if next_line.is_none() {
            if wanted {
                // A bad line after the first ends this page; the next call reports its error.
                let entry = match numbered_line(number, &buffer, scanned.clipped) {
                    Ok(entry) => entry,
                    Err(_) if !text.is_empty() => String::new(),
                    Err(e) => return Err(e),
                };
                let fits = !entry.is_empty() && text.len() + entry.len() <= READ_TEXT_LIMIT;
                if text.is_empty() || fits {
                    text.push_str(&entry);
                    taken += 1;
                    end_line = Some(number);
                    crlf |= scanned.crlf;
                    continue;
                }
            }
            next_line = Some(number);
        }
        if !count_all {
            break;
        }
    }
    let mut result = json!({
        "text":text,
        "start_line":line,
        "end_line":end_line,
        "next_line":next_line,
        "truncated":next_line.is_some(),
    });
    if count_all {
        result["total_lines"] = json!(number);
    }
    if crlf {
        result["line_ending"] = json!("crlf");
    }
    Ok(result)
}
struct ScannedLine {
    clipped: bool,
    crlf: bool,
}
/// Scans one line in buffered chunks, keeping at most `keep` bytes of its content.
fn scan_line(
    reader: &mut impl BufRead,
    keep: usize,
    line: &mut Vec<u8>,
) -> AppResult<Option<ScannedLine>> {
    line.clear();
    let mut total = 0usize;
    let mut tail = [0u8; 2];
    let mut newline = false;
    loop {
        let chunk = reader.fill_buf().map_err(err)?;
        if chunk.is_empty() {
            break;
        }
        let end = chunk
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        let part = &chunk[..end];
        if part.contains(&0) {
            return Err(BINARY_CONTENT.into());
        }
        let room = keep.saturating_sub(line.len());
        line.extend_from_slice(&part[..end.min(room)]);
        for &byte in &part[end.saturating_sub(2)..] {
            tail = [tail[1], byte];
        }
        total += end;
        newline = part.last() == Some(&b'\n');
        reader.consume(end);
        if newline {
            break;
        }
    }
    if total == 0 {
        return Ok(None);
    }
    let crlf = newline && total >= 2 && tail[0] == b'\r';
    let content = total - usize::from(newline) - usize::from(crlf);
    let clipped = content > line.len();
    line.truncate(content);
    Ok(Some(ScannedLine { clipped, crlf }))
}
fn numbered_line(number: u64, bytes: &[u8], clipped: bool) -> AppResult<String> {
    let (text, clipped) = match std::str::from_utf8(bytes) {
        Ok(text) => (text, clipped),
        Err(e) => {
            let valid = std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default();
            // Only the characters that are returned must be valid; the cut part is not checked.
            let beyond_cut = valid.chars().nth(READ_LINE_CHARS - 1).is_some();
            if !beyond_cut && !(clipped && e.error_len().is_none()) {
                return Err(INVALID_UTF8.into());
            }
            (valid, true)
        }
    };
    Ok(match text.char_indices().nth(READ_LINE_CHARS) {
        Some((cut, _)) => format!("{number:>6}\t{}{LINE_CLIPPED}\n", &text[..cut]),
        None if clipped => format!("{number:>6}\t{text}{LINE_CLIPPED}\n"),
        None => format!("{number:>6}\t{text}\n"),
    })
}
pub fn image(store: &Store, scope: &str, path: &str) -> AppResult<Value> {
    use base64::Engine;
    let dir = root(store, scope)?;
    let path = relative(path, false)?;
    let mime = match path.extension().and_then(|s| s.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err("仅支持 PNG、JPEG 和 WebP 图片".into()),
    };
    let file = dir.open(&path).map_err(err)?;
    let metadata = file.metadata().map_err(err)?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
        return Err("图片必须是 2 MiB 以内的普通文件".into());
    }
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("图片超过 2 MiB".into());
    }
    Ok(json!({"mime_type":mime,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}))
}
pub fn write(store: &Store, scope: &str, path: &str, content: &str) -> AppResult<Value> {
    if content.len() > MAX_WRITE_BYTES {
        return Err("单次写入上限为 1 MiB".into());
    }
    let dir = root(store, scope)?;
    let path = relative(path, false)?;
    let _mutation = FILE_MUTATIONS.lock().unwrap();
    let parent = open_parent(&dir, &path).map_err(err)?;
    let name = path.file_name().ok_or("文件名为空")?;
    let permissions = if let Ok(meta) = parent.symlink_metadata(name) {
        if !meta.is_file() {
            return Err("目标必须是普通文件，不能直接覆盖目录或符号链接".into());
        }
        Some(meta.permissions())
    } else {
        None
    };
    atomic_write(&parent, name, content, permissions)?;
    Ok(json!({"written_bytes":content.len()}))
}
pub fn edit(
    store: &Store,
    scope: &str,
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> AppResult<Value> {
    if old_string.is_empty() {
        return Err("old_string 不能为空".into());
    }
    if old_string == new_string {
        return Err("new_string 必须与 old_string 不同".into());
    }
    let dir = root(store, scope)?;
    let path = relative(path, false)?;
    let _mutation = FILE_MUTATIONS.lock().unwrap();
    let parent = open_parent(&dir, &path).map_err(|e| format!("无法打开目标所在目录：{e}"))?;
    let name = path.file_name().ok_or("文件名为空")?;
    let metadata = parent.symlink_metadata(name).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => "目标文件不存在".into(),
        _ => format!("无法读取目标文件信息：{e}"),
    })?;
    if !metadata.is_file() {
        return Err(EDIT_TARGET_REQUIRED.into());
    }

    let file = open_edit_target(&parent, name).map_err(|e| format!("无法打开待修改文件：{e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("无法读取目标文件信息：{e}"))?;
    if !metadata.is_file() {
        return Err(EDIT_TARGET_REQUIRED.into());
    }
    if metadata.len() > MAX_WRITE_BYTES as u64 {
        return Err("待修改文件大小不能超过 1 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_WRITE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("无法读取待修改文件：{e}"))?;
    if bytes.len() > MAX_WRITE_BYTES {
        return Err("待修改文件大小不能超过 1 MiB".into());
    }
    let text = std::str::from_utf8(&bytes)
        .ok()
        .filter(|text| !text.as_bytes().contains(&0))
        .ok_or("只能修改不含 NUL 的 UTF-8 文本文件")?;

    let mut matches = text.match_indices(old_string);
    let (first_offset, _) = matches
        .next()
        .ok_or("未找到 old_string，请先 fs_read 确认当前内容")?;
    let line_number = |offset| 1 + text[..offset].bytes().filter(|&byte| byte == b'\n').count();
    let first_line = line_number(first_offset);
    let replacements = 1 + matches.clone().count();
    if replacements > 1 && !replace_all {
        let mut lines = vec![first_line.to_string()];
        lines.extend(
            matches
                .take(4)
                .map(|(offset, _)| line_number(offset).to_string()),
        );
        return Err(format!(
            "old_string 匹配到 {replacements} 处（前 {} 处位于第 {} 行）。请扩大 old_string 上下文或设置 replace_all=true",
            lines.len(),
            lines.join("、")
        ));
    }

    let written_bytes = (text.len() - replacements * old_string.len())
        .saturating_add(replacements.saturating_mul(new_string.len()));
    if written_bytes > MAX_WRITE_BYTES {
        return Err("替换后的文件大小不能超过 1 MiB".into());
    }
    let updated = text.replace(old_string, new_string);
    atomic_write(&parent, name, &updated, Some(metadata.permissions()))
        .map_err(|e| format!("无法写回待修改文件：{e}"))?;
    Ok(json!({"replacements":replacements,"written_bytes":written_bytes,"first_line":first_line}))
}
fn open_parent(dir: &Dir, path: &Path) -> io::Result<Dir> {
    dir.open_dir(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
}
#[cfg(unix)]
fn open_edit_target(parent: &Dir, name: &OsStr) -> io::Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };

    let name = CString::new(name.as_bytes())?;
    // SAFETY: The directory and C string remain valid. `name` is a single path
    // component, and O_NOFOLLOW prevents a raced symlink from being opened.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: A successful openat returns a new owned descriptor.
    Ok(File::from_std(unsafe { std::fs::File::from_raw_fd(fd) }))
}
#[cfg(windows)]
fn open_edit_target(parent: &Dir, name: &OsStr) -> io::Result<File> {
    use cap_std::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    parent.open_with(name, &options)
}
fn atomic_write(
    parent: &Dir,
    name: &OsStr,
    content: &str,
    permissions: Option<Permissions>,
) -> AppResult<()> {
    let temp = format!(".cloudmcp-{}.tmp", Uuid::new_v4());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if permissions.is_some() {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = parent.open_with(&temp, &options).map_err(err)?;
    let result = (|| {
        file.write_all(content.as_bytes()).map_err(err)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions).map_err(err)?;
        }
        file.sync_all().map_err(err)?;
        parent.rename(&temp, parent, name).map_err(err)
    })();
    if result.is_err() {
        let _ = parent.remove_file(&temp);
    }
    result
}
pub fn mutate(
    store: &Store,
    scope: &str,
    action: &str,
    path: &str,
    destination: Option<&str>,
    recursive: bool,
) -> AppResult<Value> {
    let dir = root(store, scope)?;
    let path = relative(path, false)?;
    let _mutation = FILE_MUTATIONS.lock().unwrap();
    match action {
        "mkdir" => if recursive {
            dir.create_dir_all(&path)
        } else {
            dir.create_dir(&path)
        }
        .map_err(err)?,
        "rename" => {
            let to = relative(destination.ok_or("缺少 destination")?, false)?;
            if dir.symlink_metadata(&to).is_ok() {
                return Err("目标已存在；请先明确处理目标文件".into());
            }
            dir.rename(&path, &dir, to).map_err(err)?;
        }
        "remove" => {
            let m = dir.symlink_metadata(&path).map_err(err)?;
            if m.is_dir() {
                if recursive {
                    dir.remove_dir_all(&path)
                } else {
                    dir.remove_dir(&path)
                }
                .map_err(err)?;
            } else {
                dir.remove_file(&path).map_err(err)?;
            }
        }
        _ => return Err("action 应为 mkdir、rename 或 remove".into()),
    }
    Ok(json!({"ok":true}))
}
pub fn search(store: &Store, scope: &str, path: &str, query: &str) -> AppResult<Value> {
    if query.is_empty() || query.len() > 1000 {
        return Err("搜索文本应为 1–1000 字节".into());
    }
    let dir = root(store, scope)?;
    let mut pending = vec![relative(path, true)?];
    let mut matches = Vec::new();
    let mut scanned = 0;
    let mut visited = 0;
    let mut truncated = false;
    let mut bytes = 0usize;
    while let Some(path) = pending.pop() {
        visited += 1;
        if visited >= 4000 || scanned >= 3000 || matches.len() >= 80 || bytes >= 16 * 1024 * 1024 {
            truncated = true;
            break;
        }
        let Ok(meta) = dir.symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            for e in dir.read_dir(&path).map_err(err)?.take(4000).flatten() {
                let name = e.file_name();
                if matches!(
                    name.to_str(),
                    Some(".git" | "node_modules" | "target" | "dist" | ".codex-runs")
                ) {
                    continue;
                }
                if pending.len() < 4000 {
                    pending.push(path.join(name));
                } else {
                    truncated = true;
                }
            }
        } else if meta.is_file() && meta.len() <= 512 * 1024 {
            scanned += 1;
            let Ok(mut f) = dir.open(&path) else { continue };
            let mut data = Vec::new();
            if Read::by_ref(&mut f)
                .take(512 * 1024)
                .read_to_end(&mut data)
                .is_err()
            {
                continue;
            }
            bytes += data.len();
            if data.contains(&0) {
                continue;
            }
            for (index, line) in String::from_utf8_lossy(&data).lines().enumerate() {
                if line.contains(query) {
                    matches.push(json!({"path":path.to_string_lossy(),"line":index+1,"text":clipped(line,240)}));
                    if matches.len() >= 80 {
                        break;
                    }
                }
            }
        }
    }
    Ok(
        json!({"matches":matches,"scanned_files":scanned,"truncated":truncated || !pending.is_empty(),"ignored":[".git","node_modules","target","dist",".codex-runs","symlinks","files larger than 512 KiB"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn workspace() -> (tempfile::TempDir, Arc<Store>, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        store.add_scope(&project, Some("project")).unwrap();
        (temp, store, project)
    }

    #[test]
    fn edit_replaces_one_exact_match_and_preserves_permissions() {
        let (_temp, store, project) = workspace();
        fs::create_dir(project.join("src")).unwrap();
        let path = project.join("src/file.txt");
        fs::write(&path, "头部\r\n  old\r\ntail\r\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
        }
        let permissions = fs::metadata(&path).unwrap().permissions();

        let result = edit(
            &store,
            "project",
            "src/file.txt",
            "  old\r\n",
            "  新值\r\n",
            false,
        )
        .unwrap();
        let expected = "头部\r\n  新值\r\ntail\r\n";
        assert_eq!(
            result,
            json!({"replacements":1,"written_bytes":expected.len(),"first_line":2})
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(fs::metadata(&path).unwrap().permissions(), permissions);
    }

    #[test]
    fn edit_reports_missing_match_without_normalizing_newlines() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        fs::write(&path, "old\r\n").unwrap();

        let error = edit(&store, "project", "file.txt", "old\n", "new\n", false).unwrap_err();
        assert!(error.contains("未找到 old_string"));
        assert!(error.contains("请先 fs_read 确认当前内容"));
        assert_eq!(fs::read_to_string(path).unwrap(), "old\r\n");
    }

    #[test]
    fn edit_reports_match_count_and_first_five_lines() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        let contents = format!("header\n{}", "old\n".repeat(7));
        fs::write(&path, &contents).unwrap();

        let error = edit(&store, "project", "file.txt", "old", "new", false).unwrap_err();
        assert!(error.contains("7 处"));
        assert!(error.contains("第 2、3、4、5、6 行"));
        assert!(error.contains("扩大 old_string 上下文或设置 replace_all=true"));
        assert_eq!(fs::read_to_string(path).unwrap(), contents);
    }

    #[test]
    fn edit_replaces_all_non_overlapping_matches() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        fs::write(&path, "header\nxxx\nxx\n").unwrap();

        let result = edit(&store, "project", "file.txt", "xx", "", true).unwrap();
        let expected = "header\nx\n\n";
        assert_eq!(
            result,
            json!({"replacements":2,"written_bytes":expected.len(),"first_line":2})
        );
        assert_eq!(fs::read_to_string(path).unwrap(), expected);
    }

    #[test]
    fn edit_rejects_empty_old_string_and_identical_strings() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        fs::write(&path, "old").unwrap();

        assert!(edit(&store, "project", "file.txt", "", "new", true)
            .unwrap_err()
            .contains("old_string 不能为空"));
        assert!(edit(&store, "project", "file.txt", "old", "old", false)
            .unwrap_err()
            .contains("new_string 必须与 old_string 不同"));
        assert_eq!(fs::read_to_string(path).unwrap(), "old");
    }

    #[test]
    fn edit_rejects_missing_files_and_directories() {
        let (_temp, store, project) = workspace();
        fs::create_dir(project.join("dir")).unwrap();

        assert!(edit(&store, "project", "missing.txt", "old", "new", false)
            .unwrap_err()
            .contains("目标文件不存在"));
        assert!(!project.join("missing.txt").exists());
        assert!(edit(&store, "project", "dir", "old", "new", false)
            .unwrap_err()
            .contains("普通文件"));
    }

    #[cfg(unix)]
    #[test]
    fn edit_rejects_symlink_to_file_within_workspace() {
        let (_temp, store, project) = workspace();
        let target = project.join("target.txt");
        let link = project.join("link.txt");
        fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink("target.txt", &link).unwrap();

        let error = edit(&store, "project", "link.txt", "old", "new", false).unwrap_err();
        assert!(error.contains("符号链接"));
        assert!(fs::symlink_metadata(link).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(target).unwrap(), "old");
    }

    #[test]
    fn edit_rejects_nul_bytes_and_invalid_utf8() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        for contents in [b"old\0".as_slice(), b"old\xff".as_slice()] {
            fs::write(&path, contents).unwrap();
            assert!(edit(&store, "project", "file.txt", "old", "new", false)
                .unwrap_err()
                .contains("UTF-8"));
            assert_eq!(fs::read(&path).unwrap(), contents);
        }
    }

    #[test]
    fn edit_rejects_parent_path_components() {
        let (_temp, store, project) = workspace();
        fs::create_dir(project.join("dir")).unwrap();
        let path = project.join("file.txt");
        fs::write(&path, "old").unwrap();

        let error = edit(&store, "project", "dir/../file.txt", "old", "new", false).unwrap_err();
        assert!(error.contains("不能包含 .."));
        assert_eq!(fs::read_to_string(path).unwrap(), "old");
    }

    #[test]
    fn edit_enforces_input_and_output_byte_limits() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        let contents = "a".repeat(MAX_WRITE_BYTES);
        fs::write(&path, format!("{contents}b")).unwrap();
        assert!(edit(&store, "project", "file.txt", "b", "", false)
            .unwrap_err()
            .contains("待修改文件大小不能超过 1 MiB"));

        fs::write(&path, &contents).unwrap();
        assert!(edit(&store, "project", "file.txt", "a", "éi", true)
            .unwrap_err()
            .contains("替换后的文件大小不能超过 1 MiB"));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);

        let result = edit(&store, "project", "file.txt", "a", "b", true).unwrap();
        assert_eq!(
            result,
            json!({"replacements":MAX_WRITE_BYTES,"written_bytes":MAX_WRITE_BYTES,"first_line":1})
        );
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "b".repeat(MAX_WRITE_BYTES)
        );
    }

    #[test]
    fn write_still_creates_and_replaces_files() {
        let (_temp, store, project) = workspace();
        let path = project.join("file.txt");
        assert_eq!(
            write(&store, "project", "file.txt", "old").unwrap(),
            json!({"written_bytes":3})
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(
            write(&store, "project", "file.txt", "新内容").unwrap(),
            json!({"written_bytes":"新内容".len()})
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "新内容");
    }
    fn read_text(result: &Value) -> &str {
        result["text"].as_str().unwrap()
    }

    #[test]
    fn read_lines_returns_numbered_pages() {
        let (_temp, store, project) = workspace();
        let body: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        fs::write(project.join("file.txt"), body).unwrap();

        let result = read_lines(&store, "project", "file.txt", 3, Some(4)).unwrap();
        assert_eq!(
            read_text(&result),
            "     3\tline 3\n     4\tline 4\n     5\tline 5\n     6\tline 6\n"
        );
        assert_eq!(result["start_line"], json!(3));
        assert_eq!(result["end_line"], json!(6));
        assert_eq!(result["next_line"], json!(7));
        assert_eq!(result["truncated"], json!(true));
        assert_eq!(result["total_lines"], json!(10));

        let last = read_lines(&store, "project", "file.txt", 9, None).unwrap();
        assert_eq!(read_text(&last), "     9\tline 9\n    10\tline 10\n");
        assert_eq!(last["next_line"], Value::Null);
        assert_eq!(last["truncated"], json!(false));
    }

    #[test]
    fn read_lines_past_the_end_is_empty() {
        let (_temp, store, project) = workspace();
        fs::write(project.join("file.txt"), "a\nb").unwrap();
        let result = read_lines(&store, "project", "file.txt", 5, None).unwrap();
        assert_eq!(read_text(&result), "");
        assert_eq!(result["end_line"], Value::Null);
        assert_eq!(result["next_line"], Value::Null);
        assert_eq!(result["total_lines"], json!(2));
        assert!(read_lines(&store, "project", "file.txt", 0, None).is_err());
    }

    #[test]
    fn read_lines_clips_long_lines_and_reports_crlf() {
        let (_temp, store, project) = workspace();
        let long = "长".repeat(READ_LINE_CHARS + 5);
        fs::write(project.join("file.txt"), format!("{long}\r\nshort\r\n")).unwrap();
        let result = read_lines(&store, "project", "file.txt", 1, None).unwrap();
        let expected = format!(
            "     1\t{}…[行已截断]\n     2\tshort\n",
            "长".repeat(READ_LINE_CHARS)
        );
        assert_eq!(read_text(&result), expected);
        assert_eq!(result["line_ending"], json!("crlf"));
    }

    #[test]
    fn read_lines_stops_at_the_byte_limit_on_a_line_boundary() {
        let (_temp, store, project) = workspace();
        let line = "x".repeat(1000);
        let body: String = (0..100).map(|_| format!("{line}\n")).collect();
        fs::write(project.join("file.txt"), body).unwrap();
        let result = read_lines(&store, "project", "file.txt", 1, Some(2000)).unwrap();
        let text = read_text(&result);
        assert!(text.len() <= READ_TEXT_LIMIT);
        assert!(text.ends_with('\n'));
        let end = result["end_line"].as_u64().unwrap();
        assert_eq!(text.lines().count() as u64, end);
        assert_eq!(result["next_line"], json!(end + 1));
        assert_eq!(result["total_lines"], json!(100));
    }

    #[test]
    fn read_lines_rejects_binary_files() {
        let (_temp, store, project) = workspace();
        fs::write(project.join("file.bin"), b"a\n\0b\n").unwrap();
        let error = read_lines(&store, "project", "file.bin", 1, None).unwrap_err();
        assert!(error.contains("二进制"));
    }

    #[test]
    fn read_byte_mode_is_unchanged() {
        let (_temp, store, project) = workspace();
        fs::write(project.join("file.txt"), "abcdef").unwrap();
        assert_eq!(
            read(&store, "project", "file.txt", 2, 3).unwrap(),
            json!({"text":"cde","offset":2,"next_offset":5,"total_bytes":6,"truncated":true})
        );
    }
    #[test]
    fn read_lines_bounds_very_long_lines() {
        let (_temp, store, project) = workspace();
        let long = "x".repeat(200_000);
        fs::write(project.join("file.txt"), format!("{long}\nend")).unwrap();
        let result = read_lines(&store, "project", "file.txt", 1, Some(1)).unwrap();
        let expected = format!("     1\t{}…[行已截断]\n", "x".repeat(READ_LINE_CHARS));
        assert_eq!(read_text(&result), expected);
        assert_eq!(result["next_line"], json!(2));
        assert_eq!(result["total_lines"], json!(2));

        let wide = "长".repeat(10_000);
        fs::write(project.join("wide.txt"), wide).unwrap();
        let result = read_lines(&store, "project", "wide.txt", 1, None).unwrap();
        let expected = format!("     1\t{}…[行已截断]\n", "长".repeat(READ_LINE_CHARS));
        assert_eq!(read_text(&result), expected);
    }

    #[test]
    fn read_lines_rejects_invalid_utf8() {
        let (_temp, store, project) = workspace();
        fs::write(project.join("file.txt"), b"a\xff\nb\n").unwrap();
        let error = read_lines(&store, "project", "file.txt", 1, None).unwrap_err();
        assert!(error.contains("UTF-8"));
        let result = read_lines(&store, "project", "file.txt", 2, None).unwrap();
        assert_eq!(read_text(&result), "     2\tb\n");

        fs::write(project.join("pages.txt"), b"ok\nbad\xff\n").unwrap();
        let result = read_lines(&store, "project", "pages.txt", 1, None).unwrap();
        assert_eq!(read_text(&result), "     1\tok\n");
        assert_eq!(result["next_line"], json!(2));
        assert!(read_lines(&store, "project", "pages.txt", 2, None).is_err());

        let edge = "x".repeat(READ_LINE_CHARS - 1);
        let bytes = [edge.as_bytes(), b"\xff\n".as_slice()].concat();
        fs::write(project.join("before.txt"), bytes).unwrap();
        assert!(read_lines(&store, "project", "before.txt", 1, None).is_err());

        let full = "x".repeat(READ_LINE_CHARS);
        let bytes = [full.as_bytes(), b"\xff\n".as_slice()].concat();
        fs::write(project.join("after.txt"), bytes).unwrap();
        let result = read_lines(&store, "project", "after.txt", 1, None).unwrap();
        assert_eq!(read_text(&result), format!("     1\t{full}…[行已截断]\n"));
    }

    #[test]
    fn read_lines_reports_crlf_only_for_returned_lines() {
        let (_temp, store, project) = workspace();
        fs::write(project.join("file.txt"), "a\nb\r\n").unwrap();
        let first = read_lines(&store, "project", "file.txt", 1, Some(1)).unwrap();
        assert_eq!(first.get("line_ending"), None);
        let second = read_lines(&store, "project", "file.txt", 2, Some(1)).unwrap();
        assert_eq!(read_text(&second), "     2\tb\n");
        assert_eq!(second["line_ending"], json!("crlf"));
    }
}
