//! Windows 原生应用顶栏；不创建 WebView，不向远程页面开放窗口操作。
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED, ODT_BUTTON, WM_MOUSELEAVE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::appearance::{AppBarAppearance, ThemeSnapshot, TitlebarTheme};

// 用户要求此处作为放置程序标题及后续程序菜单的原生应用顶栏，固定 36 个逻辑像素。
// desktop.rs 的网页起点共用此高度，DPI 改变时同步调整，菜单本次不预设内容。
pub const HEIGHT: f64 = 36.0;
const BAR_ID: i32 = 0x4345;
const MINIMIZE: i32 = 0x4346;
const MAXIMIZE: i32 = 0x4347;
const CLOSE: i32 = 0x4348;
const BUTTONS: [i32; 3] = [MINIMIZE, MAXIMIZE, CLOSE];
const CLASS: windows_sys::core::PCWSTR = windows_sys::core::w!("CodexElvesRemoteAppBar");

struct Bar {
    parent: HWND,
    title: Vec<u16>,
    appearance: AppBarAppearance,
    font: HFONT,
    scale: f64,
    hover: i32,
    active: bool,
    buttons: [HWND; 3],
}

impl Drop for Bar {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
        }
    }
}

fn native_error(message: &str) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::anyhow!(
        "{message}：{}",
        std::io::Error::last_os_error()
    ))
}

fn px(logical: f64, scale: f64) -> i32 {
    (logical * scale).round().max(1.0) as i32
}

fn color(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) | u32::from(rgb[1]) << 8 | u32::from(rgb[2]) << 16
}

fn blend(background: [u8; 3], foreground: [u8; 3], amount: f64) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f64::from(background[i]) * (1.0 - amount) + f64::from(foreground[i]) * amount).round()
            as u8
    })
}

unsafe fn state(hwnd: HWND) -> *mut Bar {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Bar }
}

unsafe fn redraw(hwnd: HWND) {
    unsafe {
        RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
    }
}

pub fn install(window: &tauri::Window, title: &str, close_to_tray: bool) -> tauri::Result<()> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let registered = REGISTERED.get_or_init(|| unsafe {
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(bar_proc),
            hInstance: GetModuleHandleW(null()),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class) != 0
    });
    if !registered {
        return Err(native_error("无法注册原生应用顶栏"));
    }
    let parent = window.hwnd()?.0 as HWND;
    let scale = window.scale_factor()?;
    let dark = window
        .theme()
        .is_ok_and(|theme| theme == tauri::Theme::Dark);
    unsafe {
        let hwnd = CreateWindowExW(
            0,
            CLASS,
            null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            1,
            px(HEIGHT, scale),
            parent,
            BAR_ID as usize as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return Err(native_error("无法创建原生应用顶栏"));
        }
        let data = Box::into_raw(Box::new(Bar {
            parent,
            title: title.encode_utf16().collect(),
            appearance: AppBarAppearance::new(dark),
            font: null_mut(),
            scale,
            hover: 0,
            active: true,
            buttons: [null_mut(); 3],
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
        for (index, name) in [
            windows_sys::core::w!("最小化"),
            windows_sys::core::w!("最大化"),
            if close_to_tray {
                windows_sys::core::w!("关闭到托盘")
            } else {
                windows_sys::core::w!("关闭")
            },
        ]
        .into_iter()
        .enumerate()
        {
            // 使用真正的 Windows 按钮，保留键盘按下、焦点及无障碍名称；仅绘制主题外观。
            let button = CreateWindowExW(
                0,
                windows_sys::core::w!("BUTTON"),
                name,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                0,
                1,
                1,
                hwnd,
                BUTTONS[index] as usize as HMENU,
                GetModuleHandleW(null()),
                null(),
            );
            if button.is_null()
                || SetWindowSubclass(button, Some(button_proc), 1, hwnd as usize) == 0
            {
                let error = native_error("无法创建原生窗口按钮");
                DestroyWindow(hwnd);
                return Err(error);
            }
            (*data).buttons[index] = button;
        }
    }
    Ok(())
}

pub fn resize(window: &tauri::Window, width: u32, height: u32, scale: f64) -> tauri::Result<()> {
    let parent = window.hwnd()?.0 as HWND;
    unsafe {
        let hwnd = GetDlgItem(parent, BAR_ID);
        let data = state(hwnd);
        if data.is_null() {
            return Err(native_error("原生应用顶栏尚未就绪"));
        }
        if (*data).font.is_null() || (*data).scale != scale {
            let font = CreateFontW(
                -px(12.0, scale),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                windows_sys::core::w!("Segoe UI"),
            );
            if font.is_null() {
                return Err(native_error("无法创建应用顶栏字体"));
            }
            let previous = std::mem::replace(&mut (*data).font, font);
            (*data).scale = scale;
            if !previous.is_null() {
                DeleteObject(previous);
            }
        }
        if MoveWindow(hwnd, 0, 0, width as i32, height as i32, 0) == 0 {
            return Err(native_error("无法调整应用顶栏尺寸"));
        }
        let buttons = (*data).buttons;
        let button_width = px(46.0, scale);
        // 沿用原窗口按钮 46px 宽度及底部 1px 分隔线，三枚按钮固定在右侧。
        for (index, button) in buttons.into_iter().enumerate() {
            MoveWindow(
                button,
                (width as i32 - button_width * (3 - index as i32)).max(0),
                0,
                button_width,
                (height as i32 - px(1.0, scale)).max(0),
                0,
            );
        }
        SetWindowTextW(
            buttons[1],
            if IsZoomed(parent) != 0 {
                windows_sys::core::w!("还原")
            } else {
                windows_sys::core::w!("最大化")
            },
        );
        redraw(hwnd);
    }
    Ok(())
}

pub fn update_theme(window: &tauri::Window, snapshot: ThemeSnapshot) -> tauri::Result<()> {
    let target = window.clone();
    // 颜色沿用页面实际渐变帧；只切到原生 UI 线程重绘，禁止另设 300ms 动画造成二次延迟。
    window.run_on_main_thread(move || unsafe {
        let Ok(parent) = target.hwnd() else { return };
        let hwnd = GetDlgItem(parent.0 as HWND, BAR_ID);
        let data = state(hwnd);
        if !data.is_null() && (*data).appearance.apply(snapshot) {
            redraw(hwnd);
        }
    })
}

pub fn set_active(window: &tauri::Window, active: bool) {
    if let Ok(parent) = window.hwnd() {
        unsafe {
            let hwnd = GetDlgItem(parent.0 as HWND, BAR_ID);
            let data = state(hwnd);
            if !data.is_null() {
                (*data).active = active;
                redraw(hwnd);
            }
        }
    }
}

unsafe fn fill(hdc: HDC, rect: &RECT, rgb: [u8; 3]) {
    unsafe {
        let brush = CreateSolidBrush(color(rgb));
        if !brush.is_null() {
            FillRect(hdc, rect, brush);
            DeleteObject(brush);
        }
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let data = state(hwnd);
        let mut ps = PAINTSTRUCT::default();
        let output = BeginPaint(hwnd, &mut ps);
        if !data.is_null() && !output.is_null() {
            let (theme, scale, font, parent, active, title) = (
                (*data).appearance.theme.clone(),
                (*data).scale,
                (*data).font,
                (*data).parent,
                (*data).active,
                (*data).title.clone(),
            );
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let memory = CreateCompatibleDC(output);
            let bitmap = CreateCompatibleBitmap(output, rect.right.max(1), rect.bottom.max(1));
            if !memory.is_null() && !bitmap.is_null() {
                let previous = SelectObject(memory, bitmap);
                fill(memory, &rect, theme.background);
                let line = RECT {
                    top: (rect.bottom - px(1.0, scale)).max(0),
                    ..rect
                };
                fill(memory, &line, theme.border);
                let icon_size = px(16.0, scale);
                let mut icon = SendMessageW(parent, WM_GETICON, ICON_SMALL2 as usize, 0) as HICON;
                if icon.is_null() {
                    icon = GetClassLongPtrW(parent, GCLP_HICONSM) as HICON;
                }
                if !icon.is_null() {
                    DrawIconEx(
                        memory,
                        px(10.0, scale),
                        (rect.bottom - icon_size) / 2,
                        icon,
                        icon_size,
                        icon_size,
                        0,
                        null_mut(),
                        DI_NORMAL,
                    );
                }
                let mut label = RECT {
                    left: px(10.0 + 16.0 + 7.0, scale),
                    top: 0,
                    right: (rect.right - px(46.0, scale) * 3 - px(8.0, scale)).max(0),
                    bottom: line.top,
                };
                let old_font = if !font.is_null() {
                    SelectObject(memory, font)
                } else {
                    null_mut()
                };
                SetBkMode(memory, TRANSPARENT as i32);
                SetTextColor(
                    memory,
                    color(if active {
                        theme.foreground
                    } else {
                        blend(theme.background, theme.foreground, 0.6)
                    }),
                );
                DrawTextW(
                    memory,
                    title.as_ptr(),
                    title.len() as i32,
                    &mut label,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                if !old_font.is_null() {
                    SelectObject(memory, old_font);
                }
                BitBlt(output, 0, 0, rect.right, rect.bottom, memory, 0, 0, SRCCOPY);
                SelectObject(memory, previous);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory.is_null() {
                DeleteDC(memory);
            }
        }
        EndPaint(hwnd, &ps);
    }
}

unsafe fn draw_button(hwnd: HWND, item: &DRAWITEMSTRUCT) {
    unsafe {
        let data = state(hwnd);
        if data.is_null() {
            return;
        }
        let Bar {
            parent,
            scale,
            hover,
            ..
        } = *data;
        let TitlebarTheme {
            background,
            foreground,
            dark,
            ..
        } = (*data).appearance.theme;
        let pressed = item.itemState & ODS_SELECTED != 0;
        let hot = hover == item.CtlID as i32;
        let (background, foreground) = if item.CtlID as i32 == CLOSE && (hot || pressed) {
            ([196, 43, 28], [255; 3])
        } else {
            (
                if hot || pressed {
                    blend(
                        background,
                        if dark { [255; 3] } else { [0; 3] },
                        if pressed { 0.13 } else { 0.07 },
                    )
                } else {
                    background
                },
                foreground,
            )
        };
        fill(item.hDC, &item.rcItem, background);
        let pen = CreatePen(PS_SOLID, px(1.0, scale), color(foreground));
        if pen.is_null() {
            return;
        }
        let previous_pen = SelectObject(item.hDC, pen);
        let previous_brush = SelectObject(item.hDC, GetStockObject(HOLLOW_BRUSH));
        let width = px(10.0, scale);
        let left = (item.rcItem.right + item.rcItem.left - width) / 2;
        let top = (item.rcItem.bottom + item.rcItem.top - width) / 2;
        match item.CtlID as i32 {
            MINIMIZE => {
                MoveToEx(item.hDC, left, top + width / 2, null_mut());
                LineTo(item.hDC, left + width, top + width / 2);
            }
            MAXIMIZE if IsZoomed(parent) != 0 => {
                let offset = px(2.0, scale);
                Rectangle(
                    item.hDC,
                    left + offset,
                    top,
                    left + width,
                    top + width - offset,
                );
                fill(
                    item.hDC,
                    &RECT {
                        left,
                        top: top + offset,
                        right: left + width - offset,
                        bottom: top + width,
                    },
                    background,
                );
                Rectangle(
                    item.hDC,
                    left,
                    top + offset,
                    left + width - offset,
                    top + width,
                );
            }
            MAXIMIZE => {
                Rectangle(item.hDC, left, top, left + width, top + width);
            }
            CLOSE => {
                MoveToEx(item.hDC, left, top, null_mut());
                LineTo(item.hDC, left + width, top + width);
                MoveToEx(item.hDC, left + width, top, null_mut());
                LineTo(item.hDC, left, top + width);
            }
            _ => {}
        }
        SelectObject(item.hDC, previous_brush);
        SelectObject(item.hDC, previous_pen);
        DeleteObject(pen);
        if item.itemState & ODS_FOCUS != 0 && item.itemState & ODS_NOFOCUSRECT == 0 {
            let mut focus = item.rcItem;
            InflateRect(&mut focus, -px(2.0, scale), -px(2.0, scale));
            DrawFocusRect(item.hDC, &focus);
        }
    }
}

unsafe fn resize_edge(parent: HWND, scale: f64, location: LPARAM) -> bool {
    unsafe {
        if IsZoomed(parent) != 0 {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(parent, &mut rect) == 0 {
            return false;
        }
        let x = location as i16 as i32;
        let y = (location >> 16) as i16 as i32;
        let edge = px(6.0, scale);
        x < rect.left + edge || x >= rect.right - edge || y < rect.top + edge
    }
}

unsafe extern "system" fn bar_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_PAINT => {
                paint(hwnd);
                return 0;
            }
            WM_ERASEBKGND | WM_CONTEXTMENU => return 1,
            WM_DRAWITEM if lparam != 0 => {
                let item = &*(lparam as *const DRAWITEMSTRUCT);
                if item.CtlType == ODT_BUTTON && BUTTONS.contains(&(item.CtlID as i32)) {
                    draw_button(hwnd, item);
                    return 1;
                }
            }
            WM_NCDESTROY => {
                let data = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Bar;
                if !data.is_null() {
                    drop(Box::from_raw(data));
                }
            }
            _ => {}
        }
        let data = state(hwnd);
        if !data.is_null() {
            let parent = (*data).parent;
            match message {
                WM_COMMAND if (wparam >> 16) == BN_CLICKED as usize => {
                    let command = match wparam as u16 as i32 {
                        MINIMIZE => SC_MINIMIZE,
                        MAXIMIZE if IsZoomed(parent) != 0 => SC_RESTORE,
                        MAXIMIZE => SC_MAXIMIZE,
                        CLOSE => SC_CLOSE,
                        _ => return DefWindowProcW(hwnd, message, wparam, lparam),
                    };
                    // SC_CLOSE 保留 Tauri CloseRequested：主窗口收进托盘，独立会话窗口只关闭自身。
                    PostMessageW(parent, WM_SYSCOMMAND, command as usize, 0);
                    return 0;
                }
                WM_LBUTTONDOWN => {
                    ReleaseCapture();
                    SendMessageW(
                        parent,
                        WM_NCLBUTTONDOWN,
                        HTCAPTION as usize,
                        GetMessagePos() as isize,
                    );
                    return 0;
                }
                WM_LBUTTONDBLCLK => {
                    PostMessageW(
                        parent,
                        WM_SYSCOMMAND,
                        if IsZoomed(parent) != 0 {
                            SC_RESTORE
                        } else {
                            SC_MAXIMIZE
                        } as usize,
                        0,
                    );
                    return 0;
                }
                WM_NCHITTEST if resize_edge(parent, (*data).scale, lparam) => {
                    return HTTRANSPARENT as isize;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    reference: usize,
) -> LRESULT {
    unsafe {
        let bar = reference as HWND;
        let data = state(bar);
        if message == WM_ERASEBKGND {
            return 1;
        }
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(button_proc), 1);
        } else if !data.is_null() {
            let id = GetDlgCtrlID(hwnd);
            match message {
                WM_MOUSEMOVE => {
                    if (*data).hover != id {
                        (*data).hover = id;
                        redraw(bar);
                    }
                    TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    });
                }
                WM_MOUSELEAVE => {
                    if (*data).hover == id {
                        (*data).hover = 0;
                        redraw(bar);
                    }
                }
                WM_NCHITTEST if resize_edge((*data).parent, (*data).scale, lparam) => {
                    return HTTRANSPARENT as isize;
                }
                _ => {}
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}
