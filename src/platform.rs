//! Backend-neutral surface/event model shared by the X11 and Wayland backends.
use crate::gfx::{Canvas, Color};

pub type SurfId = u32;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Panel/dock: reserves screen space (unless exclusive == 0), edge-anchored.
    Panel,
    /// Floating popup at absolute output-relative coordinates.
    Popup,
    /// Invisible full-output surface that only catches clicks (dismiss popups).
    Catcher,
    /// Full-size overlay with keyboard focus (app grid).
    Overlay,
}

#[derive(Clone, Debug)]
pub struct SurfSpec {
    pub kind: Kind,
    pub out: usize,
    pub edge: Edge,
    /// For Panel: thickness is `h` (horizontal edge) or `w` (vertical); length spans the output unless `len` > 0.
    /// For Popup/Overlay: position relative to the output origin.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Panel length along the edge (0 = full output).
    pub len: i32,
    /// Pixels the compositor/WM should keep free (0 = float over windows).
    pub exclusive: i32,
    /// Stay visible above fullscreen windows.
    pub above_fullscreen: bool,
    pub keyboard: bool,
    /// Distance from the anchored edge (panels stacked on the same edge, X11 only; Wayland stacks via exclusive zones).
    pub inset: i32,
}

impl SurfSpec {
    pub fn new(kind: Kind) -> SurfSpec {
        SurfSpec { kind, out: 0, edge: Edge::Top, x: 0, y: 0, w: 0, h: 0, len: 0, exclusive: 0, above_fullscreen: false, keyboard: false, inset: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub name: String,
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WinInfo {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub active: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub workspace: i32, // -1 = all
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ptr {
    Enter,
    Leave,
    Move,
    Down(u8),
    Up(u8),
    Scroll(i32),
}

#[derive(Debug)]
pub enum Ev {
    Pointer { surf: SurfId, x: f32, y: f32, what: Ptr },
    Key { surf: SurfId, sym: u32, text: String, ctrl: bool, mods: u8 },
    Shortcut { id: u32 },
    /// Compositor assigned a (new) size; UI must redraw.
    Configure { surf: SurfId, w: i32, h: i32 },
    Redraw { surf: SurfId },
    Closed { surf: SurfId },
    OutputsChanged,
    WindowsChanged,
    ThemeHint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayMode {
    pub id: u32,
    pub w: i32,
    pub h: i32,
    pub refresh: f32,
    pub current: bool,
    pub preferred: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayOut {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub connected: bool,
    pub primary: bool,
    /// RandR rotation bits: 1=0°, 2=90°, 4=180°, 8=270°
    pub rotation: u16,
    pub modes: Vec<DisplayMode>,
    pub scale: f32,
}

pub trait Platform {
    fn name(&self) -> &'static str;
    fn fd(&self) -> i32;
    /// Read & decode pending protocol traffic. Must never panic on malformed input.
    fn dispatch(&mut self) -> Vec<Ev>;
    fn outputs(&self) -> Vec<Output>;
    fn create(&mut self, spec: &SurfSpec) -> SurfId;
    fn update(&mut self, id: SurfId, spec: &SurfSpec);
    fn destroy(&mut self, id: SurfId);
    fn set_visible(&mut self, id: SurfId, v: bool);
    /// Upload pixels. `opaque_bg` is used to flatten alpha when there is no compositor.
    fn present(&mut self, id: SurfId, cv: &Canvas, opaque_bg: Color);
    fn windows(&self) -> Vec<WinInfo>;
    fn activate(&mut self, w: u64);
    fn minimize(&mut self, w: u64);
    fn close_window(&mut self, w: u64);
    fn workspaces(&self) -> (Vec<String>, usize);
    fn switch_workspace(&mut self, i: usize);
    fn can_alpha(&self) -> bool;
    fn fullscreen_active(&self) -> bool;
    fn flush(&mut self);
    /// Night light / color temperature in Kelvin (6500 = neutral). Returns false if unsupported.
    fn set_color_temp(&mut self, _kelvin: i32) -> bool {
        false
    }
    fn set_workspace_count(&mut self, _n: usize) {}
    /// Grab the screen pixels of region (x,y,w,h) in output coordinates (opaque). None = unsupported.
    fn capture(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) -> Option<Canvas> {
        None
    }
    /// Register a global shortcut (X11 only). Returns false if unsupported.
    fn grab_shortcut(&mut self, _id: u32, _sym: u32, _mods: u8) -> bool {
        false
    }
    fn ungrab_shortcuts(&mut self) {}
    /// Query physical outputs and their modes (None = backend cannot configure displays).
    fn display_outputs(&mut self) -> Option<Vec<DisplayOut>> {
        None
    }
    fn display_set_mode(&mut self, _output: &str, _mode: u32, _rotation: u16) -> bool {
        false
    }
    /// True while a key-repeat timer needs fast polling.
    fn repeat_pending(&self) -> bool {
        false
    }
}
