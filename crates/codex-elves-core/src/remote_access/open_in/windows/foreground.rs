use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GetForegroundWindow,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, MSG, PM_NOREMOVE,
    PeekMessageW, SMTO_ABORTIFHUNG, SMTO_ERRORONEXIT, SW_RESTORE, SendMessageTimeoutW,
    SetForegroundWindow, ShowWindowAsync, WM_NULL,
};

use super::super::Application;

struct Window {
    handle: isize,
    process_id: u32,
    title: String,
}

struct Enumeration<'a> {
    owners: Option<&'a HashSet<u32>>,
    windows: Vec<Window>,
}

unsafe extern "system" fn collect(hwnd: HWND, state: LPARAM) -> BOOL {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return true.into();
    }
    let state = unsafe { &mut *(state.0 as *mut Enumeration<'_>) };
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if state
        .owners
        .is_some_and(|owners| !owners.contains(&process_id))
    {
        return true.into();
    }
    let mut title = [0u16; 1024];
    // 快照只记句柄；仅所选外部应用需要读取目录标题，不采集其他应用（包括 Codex App）的窗口内容。
    let length = if state.owners.is_some() {
        unsafe { GetWindowTextW(hwnd, &mut title) }.max(0) as usize
    } else {
        0
    };
    state.windows.push(Window {
        handle: hwnd.0 as isize,
        process_id,
        title: String::from_utf16_lossy(&title[..length]).to_lowercase(),
    });
    true.into()
}

fn windows(owners: Option<&HashSet<u32>>) -> Vec<Window> {
    let mut state = Enumeration {
        owners,
        windows: Vec::new(),
    };
    let _ = unsafe {
        EnumWindows(
            Some(collect),
            LPARAM((&mut state as *mut Enumeration<'_>) as isize),
        )
    };
    state.windows
}

pub(super) struct Snapshot {
    handles: HashSet<isize>,
    foreground: isize,
}

impl Snapshot {
    pub(super) fn capture() -> Self {
        Self {
            handles: windows(None).iter().map(|window| window.handle).collect(),
            foreground: unsafe { GetForegroundWindow() }.0 as isize,
        }
    }
}

struct InputLink(u32, u32);

impl InputLink {
    fn foreground() -> Option<Self> {
        let foreground = unsafe { GetForegroundWindow() };
        let current = unsafe { GetCurrentThreadId() };
        let owner = unsafe { GetWindowThreadProcessId(foreground, None) };
        if foreground.is_invalid() || owner == 0 || owner == current {
            return None;
        }
        // 不关联无响应的前台线程；只探测 Windows 的空消息，不能读取或调用任何应用内部对象。
        if unsafe {
            SendMessageTimeoutW(
                foreground,
                WM_NULL,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                100,
                None,
            )
        }
        .0 == 0
        {
            return None;
        }
        let mut message = MSG::default();
        let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
        unsafe { AttachThreadInput(current, owner, true) }
            .as_bool()
            .then_some(Self(current, owner))
    }
}

impl Drop for InputLink {
    fn drop(&mut self) {
        let _ = unsafe { AttachThreadInput(self.0, self.1, false) };
    }
}

fn raise(handle: isize) -> bool {
    let hwnd = HWND(handle as *mut _);
    unsafe {
        if GetForegroundWindow() == hwnd {
            return true;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        // 用户要求后台打开后将窗口显示前台；普通激活未生效时短暂关联可响应的前台输入队列，并立即解除。
        // 不模拟 Alt/鼠标输入，不修改系统前台锁定策略，不依赖 Codex App 或管理器的执行通道。
        let _link = InputLink::foreground();
        let _ = unsafe { BringWindowToTop(hwnd) };
        let _ = unsafe { SetForegroundWindow(hwnd) };
    }
    // 恢复最小化窗口是异步请求；只按实际前台窗口确认，不能将 API 调用成功当成已显示。
    for _ in 0..4 {
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn candidate_rank(fresh: bool, matching: bool, terminal: bool) -> Option<u8> {
    // PowerShell/WSL 明确使用新窗口，不能把同目录的旧终端误报为当前对话；编辑器和文件夹仍允许复用对应目录窗口。
    (if terminal {
        fresh && matching
    } else {
        fresh || matching
    })
    .then_some(matching as u8 * 4 + fresh as u8 * 2)
}

pub(super) fn activate(
    before: &Snapshot,
    application: &Application,
    terminal: Option<&Path>,
    child: Option<u32>,
    root: &Path,
) -> bool {
    let expected = application.executable.to_string_lossy().to_lowercase();
    let title = if terminal.is_some() {
        super::window_title(application, root).to_lowercase()
    } else {
        root.file_name()
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .to_lowercase()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last = None;
    loop {
        // 用户要求激活新窗口和被单实例应用复用的旧窗口；仅匹配所选程序的安装路径或其 Windows Terminal 宿主。
        // 不访问 Codex App 的连接、页面或内部对象，也不靠标题全局抢占其他软件的窗口。
        let mut owners: HashSet<u32> = crate::windows_integration::enumerate_processes()
            .into_iter()
            .filter(|process| {
                process
                    .executable_path
                    .as_ref()
                    .is_some_and(|path| path.to_string_lossy().to_lowercase() == expected)
                    || terminal.is_some()
                        && process.exe_file.eq_ignore_ascii_case("WindowsTerminal.exe")
            })
            .map(|process| process.process_id)
            .collect();
        owners.extend(child);
        let foreground = unsafe { GetForegroundWindow() };
        let mut foreground_owner = 0;
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_owner)) };
        // 打开期间用户主动切到其他软件时尊重新操作，不在重试中持续抢回焦点。
        if foreground.0 as isize != before.foreground && !owners.contains(&foreground_owner) {
            return false;
        }
        let mut candidates: Vec<_> = windows(Some(&owners))
            .into_iter()
            .filter_map(|window| {
                let fresh = !before.handles.contains(&window.handle);
                let matching = !title.is_empty() && window.title.contains(&title);
                // Explorer 和 Electron 单实例可能让多个窗口共用 PID；不能仅因进程相同就把别的目录窗口当成本次目标。
                candidate_rank(fresh, matching, terminal.is_some()).map(|rank| (rank, window))
            })
            .collect();
        candidates.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        if let Some((_, window)) = candidates.first() {
            #[cfg(test)]
            if last != Some(window.handle) {
                eprintln!(
                    "找到所选应用窗口：pid={}，新窗口={}，目录匹配={}",
                    window.process_id,
                    !before.handles.contains(&window.handle),
                    !title.is_empty() && window.title.contains(&title)
                );
            }
            last = Some(window.handle);
            if raise(window.handle) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Windows 的前台限制仍可能拒绝请求；仅提示对应窗口并如实回执，不模拟按键或修改全局焦点策略。
    if let Some(handle) = last {
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: HWND(handle as *mut _),
            dwFlags: FLASHW_TRAY,
            uCount: 2,
            ..Default::default()
        };
        let _ = unsafe { FlashWindowEx(&info) };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_terminals_cannot_be_replaced_by_existing_windows_in_the_same_directory() {
        assert_eq!(candidate_rank(false, true, true), None);
        assert_eq!(candidate_rank(true, true, true), Some(6));
        assert_eq!(candidate_rank(true, false, true), None);
        assert_eq!(candidate_rank(false, true, false), Some(4));
        assert_eq!(candidate_rank(false, false, false), None);
    }
}
