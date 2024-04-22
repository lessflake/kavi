use crate::gui::Key;

use super::types::{KeyEvent, KeyState, Modifiers};
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle, Win32Handle};
use winapi::{
    shared::{
        minwindef::{FALSE, HINSTANCE, INT, LPARAM, LRESULT, UINT, WPARAM},
        ntdef::SHORT,
        windef::HWND,
    },
    um::winuser::{
        DefWindowProcW, DestroyWindow, DispatchMessageW, GetKeyState, GetMessageW, GetMonitorInfoW,
        GetWindowLongPtrW, GetWindowLongW, GetWindowPlacement, MapVirtualKeyA, MonitorFromWindow,
        PeekMessageW, PostMessageW, PostQuitMessage, SendMessageW, SetWindowLongPtrW,
        SetWindowLongW, SetWindowPlacement, SetWindowPos, ShowWindow, TranslateMessage,
        CREATESTRUCTW, GWLP_USERDATA, GWL_STYLE, HWND_TOP, MAPVK_VK_TO_CHAR, MONITORINFO,
        MONITOR_DEFAULTTOPRIMARY, MSG, PM_NOREMOVE, /*PM_REMOVE,*/ SC_KEYMENU,
        SWP_FRAMECHANGED, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, VK_CONTROL, VK_MENU,
        VK_SHIFT, WINDOWPLACEMENT, WM_APP, WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY,
        WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_KEYUP, WM_PAINT, WM_QUIT, WM_SETREDRAW,
        WM_SIZE, WM_SYSCHAR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WS_OVERLAPPEDWINDOW,
    },
};

const REPEAT_MASK: LPARAM = 0x4000_0000;
const SCANCODE_MASK: LPARAM = 0x01ff_0000;
const MODIFIER_MASK: SHORT = 0x80;

#[derive(Debug)]
pub enum WindowEvent {
    Quit,
    RedrawRequested,
    Keyboard(KeyEvent),
    // Resize(u32, u32),
    Resize(u32, u32, Option<oneshot::Sender<()>>),
}

pub struct Window {
    pub rx: crossbeam_channel::Receiver<WindowEvent>,
    module: HINSTANCE,
    window: HWND,
}

struct InitMessage {
    hwnd: HWND,
    module: HINSTANCE,
}
unsafe impl Send for InitMessage {}
unsafe impl Sync for InitMessage {}

impl Window {
    pub fn start_with_thread(width: u32, height: u32) -> anyhow::Result<Self> {
        let (init_tx, init_rx) = crossbeam_channel::bounded(1);
        let (state, rx) = WindowState::new();
        std::thread::spawn(move || {
            let module = sys::module_handle().unwrap();
            let class = sys::register_class(module).unwrap();
            let hwnd = sys::create_window(module, class, state, width, height).unwrap();
            init_tx.send(InitMessage { hwnd, module }).unwrap();

            dispatch_messages_blocking();
        });
        let InitMessage { hwnd, module } = init_rx.recv().unwrap();
        Ok(Self {
            rx,
            module,
            window: hwnd,
        })
    }

    pub fn close(&self) {
        unsafe {
            PostMessageW(self.window, KAVI_WM_CLOSE, 0, 0);
            // PostQuitMessage(0);
        }
    }

    pub fn toggle_fullscreen(&mut self) {
        unsafe {
            PostMessageW(self.window, KAVI_WM_TOGGLE_FULLSCREEN, 0, 0);
        }
    }
}

struct WindowState {
    tx: crossbeam_channel::Sender<WindowEvent>,
    vk_stash: Option<WPARAM>,
    in_size_loop: bool,
    saved_position: Option<WINDOWPLACEMENT>,
}

impl WindowState {
    fn new() -> (Self, crossbeam_channel::Receiver<WindowEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            Self {
                tx,
                vk_stash: None,
                in_size_loop: false,
                saved_position: None,
            },
            rx,
        )
    }
}

pub fn dispatch_messages_blocking() {
    unsafe {
        let mut msg = core::mem::zeroed::<MSG>();
        log::debug!("message pump is thread {:?}", std::thread::current().id());
        while GetMessageW(&mut msg, core::ptr::null_mut(), 0, 0) > 0 {
            // println!("MESSAGE {:x}", msg.message);
            if msg.message == WM_QUIT {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe impl HasRawWindowHandle for Window {
    fn raw_window_handle(&self) -> RawWindowHandle {
        let mut handle = Win32Handle::empty();
        handle.hwnd = self.window as _;
        handle.hinstance = self.module as _;
        RawWindowHandle::Win32(handle)
    }
}

const KAVI_WM_CLOSE: UINT = WM_APP;
const KAVI_WM_TOGGLE_FULLSCREEN: UINT = WM_APP + 1;

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // println!("MESSAGE {:x}", msg);
    match msg {
        WM_CLOSE | KAVI_WM_CLOSE => {
            DestroyWindow(hwnd);
        }

        WM_CREATE => {
            let create_struct: *mut CREATESTRUCTW = lparam as *mut _;
            if create_struct.is_null() {
                return 0;
            }
            let state = (*create_struct).lpCreateParams;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as _);
            return 1;
        }
        WM_DESTROY => {
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            (&*state).tx.send(WindowEvent::Quit).unwrap();
            Box::from_raw(state);
            PostQuitMessage(0);
        }

        WM_CHAR | WM_SYSCHAR | WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            if let Some(event) = process_key(hwnd, msg, wparam, lparam) {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
                state.tx.send(WindowEvent::Keyboard(event)).unwrap();
            }

            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        WM_SYSCOMMAND => {
            if wparam == SC_KEYMENU && (lparam >> 16) <= 0 {
                return 0;
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        WM_ENTERSIZEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
            state.in_size_loop = true;
        }
        WM_EXITSIZEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
            state.in_size_loop = false;
        }
        WM_SIZE => {
            // TODO: use of static is temp
            static FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let width = (lparam & 0xffff) as u32;
            let height = (lparam >> 16) as u32;
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
            if !FLAG.load(std::sync::atomic::Ordering::Relaxed) {
                FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
                return 0;
            }
            // if state.in_size_loop {
            println!("resizing to {}x{}", width, height);
            let begin_time = std::time::Instant::now();
            let (tx, rx) = oneshot::channel();
            state
                .tx
                .send(WindowEvent::Resize(width, height, Some(tx)))
                .unwrap();
            rx.recv().unwrap(); // rendering finished hopefully
            println!("took {}us", begin_time.elapsed().as_micros());
            // std::thread::sleep_ms(255);
            // } else {
            //     state.tx.send(WindowEvent::Resize(width, height, None)).unwrap();
            // }
        }

        // 1080 scanlines happen in ~15ms
        // How many seconds per scanline is that? about 1.4 microseconds per scanline.
        WM_PAINT => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
            state.tx.send(WindowEvent::RedrawRequested).unwrap();
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        KAVI_WM_TOGGLE_FULLSCREEN => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
            state.toggle_fullscreen_internal(hwnd);
        }

        // WM_SETFOCUS => {
        //     return DefWindowProcW(hwnd, msg, wparam, lparam);
        // }
        // WM_KILLFOCUS => {
        //     return DefWindowProcW(hwnd, msg, wparam, lparam);
        // }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }

    0
}

impl WindowState {
    fn toggle_fullscreen_internal(&mut self, hwnd: HWND) {
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            if style & WS_OVERLAPPEDWINDOW != 0 {
                let mut previous = std::mem::zeroed::<WINDOWPLACEMENT>();
                let mut mi = std::mem::zeroed::<MONITORINFO>();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as _;
                if GetWindowPlacement(hwnd, &mut previous) != 0
                    && GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY), &mut mi)
                        != 0
                {
                    self.saved_position = Some(previous);
                    SendMessageW(hwnd, WM_SETREDRAW, FALSE as WPARAM, 0);
                    SetWindowLongW(hwnd, GWL_STYLE, (style & (!WS_OVERLAPPEDWINDOW)) as i32);
                    SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        mi.rcMonitor.left,
                        mi.rcMonitor.top,
                        mi.rcMonitor.right - mi.rcMonitor.left,
                        mi.rcMonitor.bottom - mi.rcMonitor.top,
                        SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                }
            } else {
                // ShowWindow(hwnd, SW_HIDE);
                SetWindowLongW(hwnd, GWL_STYLE, (style | WS_OVERLAPPEDWINDOW) as i32);
                SetWindowPlacement(hwnd, self.saved_position.as_ref().unwrap());
                // ShowWindow(hwnd, SW_SHOW);
                self.saved_position = None;
            }
        }
    }
}

unsafe fn get_modifiers() -> Modifiers {
    let mut modifiers = Modifiers::empty();

    for (vk, modifier) in [
        (VK_CONTROL, Modifiers::CONTROL),
        (VK_MENU, Modifiers::ALT),
        (VK_SHIFT, Modifiers::SHIFT),
    ] {
        if GetKeyState(vk) & MODIFIER_MASK != 0 {
            modifiers |= modifier;
        }
    }

    modifiers
}

unsafe fn process_key(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<KeyEvent> {
    match msg {
        WM_CHAR | WM_SYSCHAR => {
            if no_duplicate_inputs_queued(hwnd, msg, lparam) {
                let window_state =
                    &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
                let stash = window_state.vk_stash.take();

                let state = KeyState::Press;
                let mods = get_modifiers();
                let repeat = (lparam & REPEAT_MASK) != 0;

                Some(KeyEvent {
                    state,
                    key: vk_to_kavi_key(stash.unwrap() as INT),
                    translated: Some(char::from_u32(wparam as u32).unwrap()),
                    mods,
                    repeat,
                })
            } else {
                unimplemented!()
            }
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if no_duplicate_inputs_queued(hwnd, msg, lparam) {
                let state = KeyState::Press;
                let mods = get_modifiers();
                let repeat = (lparam & REPEAT_MASK) != 0;

                Some(KeyEvent {
                    state,
                    key: vk_to_kavi_key(wparam as INT),
                    translated: None,
                    mods,
                    repeat,
                })
            } else {
                let window_state =
                    &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState);
                window_state.vk_stash = Some(wparam);
                None
            }
        }

        WM_KEYUP | WM_SYSKEYUP => {
            let state = KeyState::Release;
            let mods = get_modifiers();
            let repeat = (lparam & REPEAT_MASK) != 0;

            Some(KeyEvent {
                state,
                key: vk_to_kavi_key(wparam as INT),
                translated: None,
                mods,
                repeat,
            })
        }

        _ => unreachable!(),
    }
}

unsafe fn no_duplicate_inputs_queued(hwnd: HWND, msg: UINT, lparam: LPARAM) -> bool {
    let filter = match msg {
        WM_KEYDOWN | WM_CHAR => WM_CHAR,
        WM_SYSKEYDOWN | WM_SYSCHAR => WM_SYSCHAR,
        _ => unreachable!(),
    };
    let mut msg = std::mem::zeroed();
    let available = PeekMessageW(&mut msg, hwnd, filter, filter, PM_NOREMOVE);
    available == 0 || msg.lParam & SCANCODE_MASK != lparam & SCANCODE_MASK
}

mod sys {
    use super::{window_proc, WindowState}; // TODO probably should rearrange later
    use winapi::{
        shared::{
            minwindef::{ATOM, HINSTANCE},
            windef::{HBRUSH, HICON, HWND},
        },
        um::{
            errhandlingapi::GetLastError,
            libloaderapi::GetModuleHandleW,
            winnt::LPCWSTR,
            winuser::{
                CreateWindowExW, LoadCursorW, RegisterClassW, ShowWindow, CW_USEDEFAULT, IDC_ARROW,
                SW_SHOW, WNDCLASSW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW,
                WS_POPUP,
            },
        },
    };

    const CLASS_NAME: &[u16] = &[0x4b, 0x61, 0x76, 0x69, 0x00];

    macro_rules! validate {
        ($ptr:ident, $msg:literal) => {
            if $ptr.is_null() {
                let code = unsafe { GetLastError() };
                anyhow::bail!(concat!("win32 e{}: ", $msg), code);
            }
        };
    }

    macro_rules! validate_zero {
        ($val:ident, $msg:literal) => {
            if $val == 0 {
                let code = unsafe { GetLastError() };
                anyhow::bail!(concat!("win32 e{}: ", $msg), code);
            }
        };
    }

    pub(super) fn module_handle() -> anyhow::Result<HINSTANCE> {
        let module = unsafe { GetModuleHandleW(core::ptr::null()) };
        validate!(module, "acquiring module handle failed");
        Ok(module)
    }

    pub(super) fn register_class(module: HINSTANCE) -> anyhow::Result<ATOM> {
        let cursor = unsafe { LoadCursorW(core::ptr::null_mut(), IDC_ARROW) };
        validate!(cursor, "loading default cursor failed");

        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: module,
            hIcon: 0 as HICON,
            hCursor: cursor,
            hbrBackground: 0 as HBRUSH,
            lpszMenuName: 0 as LPCWSTR,
            lpszClassName: CLASS_NAME.as_ptr(),
        };

        let class = unsafe { RegisterClassW(&class) };
        validate_zero!(class, "window class registration failed");

        Ok(class)
    }

    pub(super) fn create_window(
        module: HINSTANCE,
        class: ATOM,
        state: WindowState,
        width: u32,
        height: u32,
    ) -> anyhow::Result<HWND> {
        let state: *mut WindowState = Box::leak(Box::new(state));
        let window = unsafe {
            CreateWindowExW(
                0,
                class as _,
                // CLASS_NAME.as_ptr(),
                std::ptr::null(),
                // WS_POPUP is required to trigger the correct OS present mode (undocumented)
                WS_POPUP | WS_OVERLAPPEDWINDOW | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width as _,
                height as _,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                module,
                state.cast(),
            )
        };
        validate!(window, "window creation failed");

        unsafe { ShowWindow(window, SW_SHOW) };

        Ok(window)
    }
}

fn vk_to_kavi_key(vk: INT) -> Key {
    use winapi::um::winuser::*;
    match vk {
        // VK_LBUTTON => Key::MouseLeft,
        // VK_RBUTTON => Key::MouseRight,
        // VK_CANCEL => Key::Cancel,
        // VK_MBUTTON => Key::MouseMiddle,
        // VK_XBUTTON1 => Key::Mouse4,
        // VK_XBUTTON2 => Key::Mouse5,
        VK_BACK => Key::Backspace,
        VK_TAB => Key::Tab,
        VK_CLEAR => Key::Clear,
        VK_RETURN => Key::Return,
        VK_SHIFT => Key::Shift,
        VK_CONTROL => Key::Control,
        VK_MENU => Key::Alt,
        VK_PAUSE => Key::Pause,
        VK_CAPITAL => Key::Capital,
        VK_KANA => Key::Kana,
        // VK_HANGUL => Key::Hangul,
        // VK_IME_ON => Key::ImeOn,
        VK_JUNJA => Key::Junja,
        VK_FINAL => Key::Final,
        VK_HANJA => Key::Hanja,
        // VK_KANJI => Key::Kanji,
        // VK_IME_OFF => Key::ImeOff,
        VK_ESCAPE => Key::Escape,
        VK_CONVERT => Key::Convert,
        VK_NONCONVERT => Key::NonConvert,
        VK_ACCEPT => Key::Accept,
        VK_MODECHANGE => Key::ModeChange,
        VK_SPACE => Key::Space,
        VK_PRIOR => Key::Prior,
        VK_NEXT => Key::Next,
        VK_END => Key::End,
        VK_HOME => Key::Home,
        VK_LEFT => Key::Left,
        VK_UP => Key::Up,
        VK_RIGHT => Key::Right,
        VK_DOWN => Key::Down,
        VK_SELECT => Key::Select,
        VK_PRINT => Key::Print,
        VK_EXECUTE => Key::Execute,
        VK_SNAPSHOT => Key::Snapshot,
        VK_INSERT => Key::Insert,
        VK_DELETE => Key::Delete,
        VK_HELP => Key::Help,
        0x30 => Key::Key0,
        0x31 => Key::Key1,
        0x32 => Key::Key2,
        0x33 => Key::Key3,
        0x34 => Key::Key4,
        0x35 => Key::Key5,
        0x36 => Key::Key6,
        0x37 => Key::Key7,
        0x38 => Key::Key8,
        0x39 => Key::Key9,
        0x41 => Key::A,
        0x42 => Key::B,
        0x43 => Key::C,
        0x44 => Key::D,
        0x45 => Key::E,
        0x46 => Key::F,
        0x47 => Key::G,
        0x48 => Key::H,
        0x49 => Key::I,
        0x4A => Key::J,
        0x4B => Key::K,
        0x4C => Key::L,
        0x4D => Key::M,
        0x4E => Key::N,
        0x4F => Key::O,
        0x50 => Key::P,
        0x51 => Key::Q,
        0x52 => Key::R,
        0x53 => Key::S,
        0x54 => Key::T,
        0x55 => Key::U,
        0x56 => Key::V,
        0x57 => Key::W,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5A => Key::Z,
        VK_LWIN => Key::LeftWin,
        VK_RWIN => Key::RightWin,
        VK_APPS => Key::Apps,
        VK_SLEEP => Key::Sleep,
        VK_NUMPAD0 => Key::Numpad0,
        VK_NUMPAD1 => Key::Numpad1,
        VK_NUMPAD2 => Key::Numpad2,
        VK_NUMPAD3 => Key::Numpad3,
        VK_NUMPAD4 => Key::Numpad4,
        VK_NUMPAD5 => Key::Numpad5,
        VK_NUMPAD6 => Key::Numpad6,
        VK_NUMPAD7 => Key::Numpad7,
        VK_NUMPAD8 => Key::Numpad8,
        VK_NUMPAD9 => Key::Numpad9,
        VK_MULTIPLY => Key::Multiply,
        VK_ADD => Key::Add,
        VK_SEPARATOR => Key::Separator,
        VK_SUBTRACT => Key::Subtract,
        VK_DECIMAL => Key::Decimal,
        VK_DIVIDE => Key::Divide,
        VK_F1 => Key::F1,
        VK_F2 => Key::F2,
        VK_F3 => Key::F3,
        VK_F4 => Key::F4,
        VK_F5 => Key::F5,
        VK_F6 => Key::F6,
        VK_F7 => Key::F7,
        VK_F8 => Key::F8,
        VK_F9 => Key::F9,
        VK_F10 => Key::F10,
        VK_F11 => Key::F11,
        VK_F12 => Key::F12,
        VK_F13 => Key::F13,
        VK_F14 => Key::F14,
        VK_F15 => Key::F15,
        VK_F16 => Key::F16,
        VK_F17 => Key::F17,
        VK_F18 => Key::F18,
        VK_F19 => Key::F19,
        VK_F20 => Key::F20,
        VK_F21 => Key::F21,
        VK_F22 => Key::F22,
        VK_F23 => Key::F23,
        VK_F24 => Key::F24,
        VK_NUMLOCK => Key::NumLock,
        VK_SCROLL => Key::ScrollLock,
        // 0x92-96 OEM specific
        VK_LSHIFT => Key::LeftShift,
        VK_RSHIFT => Key::RightShfit,
        VK_LCONTROL => Key::LeftControl,
        VK_RCONTROL => Key::RightControl,
        VK_LMENU => Key::LeftAlt,
        VK_RMENU => Key::RightAlt,
        VK_BROWSER_BACK => Key::BrowserBack,
        VK_BROWSER_FORWARD => Key::BrowserForward,
        VK_BROWSER_REFRESH => Key::BrowserRefresh,
        VK_BROWSER_STOP => Key::BrowserStop,
        VK_BROWSER_SEARCH => Key::BrowserSearch,
        VK_BROWSER_FAVORITES => Key::BrowserFavorites,
        VK_BROWSER_HOME => Key::BrowserHome,
        VK_VOLUME_MUTE => Key::VolumeMute,
        VK_VOLUME_DOWN => Key::VolumeDown,
        VK_VOLUME_UP => Key::VolumeUp,
        VK_MEDIA_NEXT_TRACK => Key::MediaNextTrack,
        VK_MEDIA_PREV_TRACK => Key::MediaPrevTrack,
        VK_MEDIA_STOP => Key::MediaStop,
        VK_MEDIA_PLAY_PAUSE => Key::MediaPlayPause,
        VK_LAUNCH_MAIL => Key::LaunchMail,
        VK_LAUNCH_MEDIA_SELECT => Key::LaunchMediaSelect,
        VK_LAUNCH_APP1 => Key::LaunchApp1,
        VK_LAUNCH_APP2 => Key::LaunchApp2,
        VK_OEM_PLUS => Key::Plus,
        VK_OEM_COMMA => Key::Comma,
        VK_OEM_MINUS => Key::Minus,
        VK_OEM_PERIOD => Key::Period,
        VK_OEM_1 => map_text_keys(vk).unwrap(),
        VK_OEM_2 => map_text_keys(vk).unwrap(),
        VK_OEM_3 => map_text_keys(vk).unwrap(),
        VK_OEM_4 => map_text_keys(vk).unwrap(),
        VK_OEM_5 => map_text_keys(vk).unwrap(),
        VK_OEM_6 => map_text_keys(vk).unwrap(),
        VK_OEM_7 => map_text_keys(vk).unwrap(),
        // VK_OEM_8 => Used for miscellaneous characters; it can vary by keyboard.
        // 0xE1 => OEM specific
        // VK_OEM_102 => Either the angle bracket key or the backslash key on the RT 102-key keyboard
        // 0xE3-E4 => OEM specific
        VK_PROCESSKEY => Key::Process,
        // 0xE6 => OEM specific
        VK_PACKET => Key::Packet,
        // 0xE9-F5 => OEM specific
        VK_ATTN => Key::Attention,
        VK_CRSEL => Key::CursorSelect,
        VK_EXSEL => Key::ExtendSelect,
        VK_EREOF => Key::EraseEof,
        VK_PLAY => Key::Play,
        VK_ZOOM => Key::Zoom,
        VK_PA1 => Key::Pa1,
        VK_OEM_CLEAR => Key::OemClear,
        _ => Key::Unknown,
    }
}

fn map_text_keys(vk: INT) -> Option<Key> {
    let char_key = unsafe { MapVirtualKeyA(vk as UINT, MAPVK_VK_TO_CHAR) } & 0x7FFF;
    println!("{}", char::from_u32(char_key).unwrap());
    match char::from_u32(char_key) {
        Some(';') => Some(Key::Semicolon),
        Some('/') => Some(Key::Slash),
        Some('`') => Some(Key::Backtick),
        Some('[') => Some(Key::LBracket),
        Some(']') => Some(Key::RBracket),
        Some('\'') => Some(Key::Apostrophe),
        Some('\\') => Some(Key::Backslash),
        Some('#') => Some(Key::Hash),
        _ => None,
    }
}
