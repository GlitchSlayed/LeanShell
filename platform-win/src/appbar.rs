//! Windows Application Desktop Toolbar (AppBar) management.
//!
//! Provides registration (`ABM_NEW`), dimension negotiation (`ABM_QUERYPOS`, `ABM_SETPOS`),
//! and removal (`ABM_REMOVE`) with Explorer/Shell so maximized windows reserve screen space.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppBarEdge {
    Left = ABE_LEFT as isize,
    Top = ABE_TOP as isize,
    Right = ABE_RIGHT as isize,
    Bottom = ABE_BOTTOM as isize,
}

pub struct AppBar {
    hwnd: HWND,
    registered: bool,
    edge: AppBarEdge,
}

impl AppBar {
    pub fn new(hwnd: HWND, edge: AppBarEdge) -> Self {
        Self {
            hwnd,
            registered: false,
            edge,
        }
    }

    /// Registers the window as an AppBar.
    pub fn register(&mut self, callback_msg: u32) -> bool {
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: callback_msg,
            ..Default::default()
        };
        let res = unsafe { SHAppBarMessage(ABM_NEW, &mut abd) };
        self.registered = res != 0;
        self.registered
    }

    /// Negotiates position and dimensions on the display edge.
    pub fn set_pos(&mut self, rect: RECT) -> RECT {
        if !self.registered {
            return rect;
        }
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uEdge: self.edge as u32,
            rc: rect,
            ..Default::default()
        };
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            // Re-assign dimensions according to edge
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
        abd.rc
    }

    /// Unregisters the AppBar and restores the screen work area.
    pub fn unregister(&mut self) {
        if self.registered {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                ..Default::default()
            };
            unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut abd);
            }
            self.registered = false;
        }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        self.unregister();
    }
}
