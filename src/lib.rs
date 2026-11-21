// SPDX-License-Identifier: Apache-2.0
//!
//! SigmaOS-display-manager — display/output manager.
//!
//! Models display outputs (monitors), their modes (resolution/refresh),
//! arrangement (position in desktop layout), brightness, scaling, and
//! rotation. Reads real DRM/sysfs connector status where available;
//! all mutation through `DispBackend`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// A display mode (resolution + refresh).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,  // millihertz
}
impl DisplayMode {
    pub fn new(w: u32, h: u32, hz: u32) -> Self {
        Self { width: w, height: h, refresh_mhz: hz * 1000 }
    }
    pub fn label(&self) -> String {
        format!("{}x{}@{}", self.width, self.height, self.refresh_mhz / 1000)
    }
}

/// Position in desktop layout (x,y offset, can be negative).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Position { pub x: i32, pub y: i32 }

/// Rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Rotation { Normal, Right, Inverted, Left }
impl Rotation {
    pub fn degrees(&self) -> u32 {
        match self { Self::Normal=>0, Self::Right=>90, Self::Inverted=>180, Self::Left=>270 }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "normal"|"0"=>Some(Self::Normal), "right"|"90"=>Some(Self::Right),
            "inverted"|"180"=>Some(Self::Inverted), "left"|"270"=>Some(Self::Left),
            _=>None,
        }
    }
}

/// Scaling factor (1.0 = native).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scale(pub f32);
impl Scale {
    pub fn new(v: f32) -> Result<Self, DispError> {
        if v < 0.5 || v > 4.0 { return Err(DispError::BadScale(v)); }
        Ok(Self(v))
    }
}

/// A display output.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Output {
    /// connector name: HDMI-A-1, DP-1, eDP-1, VGA-1...
    pub name: String,
    pub connected: bool,
    /// physical size mm.
    pub width_mm: u32,
    pub height_mm: u32,
    /// preferred/native mode.
    pub preferred: Option<DisplayMode>,
    /// all available modes.
    pub modes: Vec<DisplayMode>,
    /// current mode.
    pub current: Option<DisplayMode>,
    /// layout position.
    pub position: Position,
    /// is primary?
    pub primary: bool,
    /// rotation.
    pub rotation: Rotation,
    /// scaling.
    pub scale: Scale,
    /// brightness 0.0-1.0.
    pub brightness: f32,
    /// manufacturer/model strings from EDID.
    pub manufacturer: String,
    pub model: String,
    /// serial.
    pub serial: String,
}
impl Output {
    pub fn aspect(&self) -> f32 {
        if self.height_mm == 0 { 0.0 } else { self.width_mm as f32 / self.height_mm as f32 }
    }
    pub fn is_laptop_panel(&self) -> bool { self.name.starts_with("eDP") || self.name.starts_with("LVDS") }
    pub fn is_external(&self) -> bool { !self.is_laptop_panel() }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DispError {
    BadScale(f32),
    BadBrightness(f32),
    NoOutput(String),
    BadMode(String),
    BadRotation(String),
    Backend(String),
    Overlap(String),
}
impl fmt::Display for DispError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadScale(v)=>write!(f,"bad scale {}",v),
            Self::BadBrightness(v)=>write!(f,"bad brightness {}",v),
            Self::NoOutput(n)=>write!(f,"no output {}",n),
            Self::BadMode(m)=>write!(f,"bad mode {}",m),
            Self::BadRotation(r)=>write!(f,"bad rotation {}",r),
            Self::Backend(e)=>write!(f,"backend: {}",e),
            Self::Overlap(m)=>write!(f,"overlap: {}",m),
        }
    }
}
impl std::error::Error for DispError {}
impl From<String> for DispError {
    fn from(e: String) -> Self { Self::Backend(e) }
}

/// Backend for display operations.
pub trait DispBackend {
    /// List connected outputs with current state.
    fn outputs(&self) -> Result<Vec<Output>, String>;
    /// Set mode on output.
    fn set_mode(&self, name: &str, mode: DisplayMode) -> Result<(), String>;
    /// Set position.
    fn set_position(&self, name: &str, pos: Position) -> Result<(), String>;
    /// Set primary.
    fn set_primary(&self, name: &str) -> Result<(), String>;
    /// Set rotation.
    fn set_rotation(&self, name: &str, rot: Rotation) -> Result<(), String>;
    /// Set scale.
    fn set_scale(&self, name: &str, scale: f32) -> Result<(), String>;
    /// Set brightness (0.0-1.0).
    fn set_brightness(&self, name: &str, level: f32) -> Result<(), String>;
    /// List brightness devices (backlight).
    fn backlights(&self) -> Result<Vec<(String, u32)>, String>;
}

/// Host backend: reads /sys/class/drm + /sys/class/backlight.
pub struct HostBackend;
impl DispBackend for HostBackend {
    fn outputs(&self) -> Result<Vec<Output>, String> {
        let mut out = vec![];
        let drm = Path::new("/sys/class/drm");
        if let Ok(entries) = std::fs::read_dir(drm) {
            for ent in entries.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                // connectors look like card0-HDMI-A-1
                if !name.contains('-') || name.contains("card") && !name.contains("-") { continue; }
                // skip non-connector entries (card0, renderD128)
                if name.starts_with("card") && !name.contains('-') { continue; }
                if name.starts_with("render") { continue; }
                let status_path = ent.path().join("status");
                let status = std::fs::read_to_string(&status_path).unwrap_or_default();
                let connected = status.trim() == "connected";
                // EDID info
                let edid_path = ent.path().join("edid");
                let (manufacturer, model, serial) = parse_edid_info(&edid_path);
                // modes from modes file? DRM sysfs doesn't expose modes directly;
                // use /sys/kernel/debug/dri or report preferred only
                let modes = vec![];
                let current = None;
                out.push(Output {
                    name: name.clone(), connected,
                    width_mm: 0, height_mm: 0,
                    preferred: None, modes, current,
                    position: Position::default(), primary: false,
                    rotation: Rotation::Normal, scale: Scale(1.0),
                    brightness: 1.0,
                    manufacturer, model, serial,
                });
            }
        }
        Ok(out)
    }
    fn set_mode(&self, _name: &str, _mode: DisplayMode) -> Result<(), String> {
        Err("mode setting requires DRM ioctl (not implemented via sysfs)".into())
    }
    fn set_position(&self, _name: &str, _pos: Position) -> Result<(), String> {
        Err("position setting requires compositor (not implemented via sysfs)".into())
    }
    fn set_primary(&self, _name: &str) -> Result<(), String> {
        Err("primary setting requires compositor".into())
    }
    fn set_rotation(&self, _name: &str, _rot: Rotation) -> Result<(), String> {
        // rotation may be settable via /sys/class/drm/*/card*-*/dpms or debugfs
        Err("rotation requires DRM (not implemented)".into())
    }
    fn set_scale(&self, _name: &str, _scale: f32) -> Result<(), String> {
        Err("scaling requires compositor".into())
    }
    fn set_brightness(&self, name: &str, level: f32) -> Result<(), String> {
        // try /sys/class/backlight/<name>/brightness
        let bl = Path::new("/sys/class/backlight").join(name);
        let max_path = bl.join("max_brightness");
        let max: u32 = std::fs::read_to_string(&max_path).ok()
            .and_then(|s| s.trim().parse().ok()).unwrap_or(100);
        let val = (level.clamp(0.0, 1.0) * max as f32) as u32;
        std::fs::write(bl.join("brightness"), val.to_string())
            .map_err(|e| format!("brightness write: {}", e))
    }
    fn backlights(&self) -> Result<Vec<(String, u32)>, String> {
        let mut out = vec![];
        let bl = Path::new("/sys/class/backlight");
        if let Ok(entries) = std::fs::read_dir(bl) {
            for ent in entries.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                let max: u32 = std::fs::read_to_string(ent.path().join("max_brightness"))
                    .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                out.push((name, max));
            }
        }
        Ok(out)
    }
}

fn parse_edid_info(edid_path: &Path) -> (String, String, String) {
    // EDID: manufacturer at bytes 8-9 (packed), model at 10-11, serial at 12-15
    let data = match std::fs::read(edid_path) { Ok(d) => d, Err(_) => return (String::new(), String::new(), String::new()) };
    if data.len() < 16 { return (String::new(), String::new(), String::new()); }
    // manufacturer ID: 2 bytes big-endian, 5-bit letters
    let mfg_id = ((data[8] as u16) << 8) | data[9] as u16;
    let c1 = ((mfg_id >> 10) & 0x1f) as u8;
    let c2 = ((mfg_id >> 5) & 0x1f) as u8;
    let c3 = (mfg_id & 0x1f) as u8;
    let mfg = if c1 > 0 && c2 > 0 && c3 > 0 {
        format!("{}{}{}", (c1 + 64) as char, (c2 + 64) as char, (c3 + 64) as char)
    } else { String::new() };
    let model = u16::from_be_bytes([data[10], data[11]]);
    let serial = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    (mfg, format!("Model {}", model), format!("{}", serial))
}

/// In-memory backend for tests.
pub struct FakeBackend {
    pub outputs: std::cell::RefCell<Vec<Output>>,
    pub backlights: Vec<(String, u32)>,
    pub brightness: std::cell::RefCell<BTreeMap<String, f32>>,
    pub calls: std::cell::RefCell<Vec<String>>,
}
impl FakeBackend {
    pub fn new() -> Self {
        Self { outputs: std::cell::RefCell::new(vec![]), backlights: vec![],
            brightness: std::cell::RefCell::new(BTreeMap::new()),
            calls: std::cell::RefCell::new(vec![]) }
    }
}
impl DispBackend for FakeBackend {
    fn outputs(&self) -> Result<Vec<Output>, String> { Ok(self.outputs.borrow().clone()) }
    fn set_mode(&self, name: &str, mode: DisplayMode) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("mode {} {}", name, mode.label()));
        if let Some(o) = self.outputs.borrow_mut().iter_mut().find(|o| o.name == name) { o.current = Some(mode); }
        Ok(())
    }
    fn set_position(&self, name: &str, pos: Position) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("pos {} {},{}", name, pos.x, pos.y));
        Ok(())
    }
    fn set_primary(&self, name: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("primary {}", name));
        Ok(())
    }
    fn set_rotation(&self, name: &str, rot: Rotation) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("rot {} {}", name, rot.degrees()));
        Ok(())
    }
    fn set_scale(&self, name: &str, scale: f32) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("scale {} {}", name, scale));
        Ok(())
    }
    fn set_brightness(&self, name: &str, level: f32) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("bright {} {}", name, level));
        self.brightness.borrow_mut().insert(name.to_string(), level);
        Ok(())
    }
    fn backlights(&self) -> Result<Vec<(String, u32)>, String> { Ok(self.backlights.clone()) }
}

/// The display manager.
pub struct DisplayManager;

impl DisplayManager {
    /// Connected outputs only.
    pub fn connected<B: DispBackend>(be: &B) -> Result<Vec<Output>, String> {
        Ok(be.outputs()?.into_iter().filter(|o| o.connected).collect())
    }

    /// All outputs including disconnected.
    pub fn all<B: DispBackend>(be: &B) -> Result<Vec<Output>, String> {
        be.outputs()
    }

    /// Get output by name.
    pub fn output<B: DispBackend>(be: &B, name: &str) -> Result<Output, DispError> {
        be.outputs().map_err(DispError::Backend)?
            .into_iter().find(|o| o.name == name)
            .ok_or_else(|| DispError::NoOutput(name.into()))
    }

    /// Set mode — validates mode exists in output's mode list (if list non-empty).
    pub fn set_mode<B: DispBackend>(be: &B, name: &str, mode: DisplayMode) -> Result<(), DispError> {
        let o = Self::output(be, name)?;
        if !o.modes.is_empty() && !o.modes.contains(&mode) {
            return Err(DispError::BadMode(mode.label()));
        }
        be.set_mode(name, mode).map_err(DispError::Backend)
    }

    /// Set brightness with validation.
    pub fn set_brightness<B: DispBackend>(be: &B, name: &str, level: f32) -> Result<(), DispError> {
        if !(0.0..=1.0).contains(&level) {
            return Err(DispError::BadBrightness(level));
        }
        be.set_brightness(name, level).map_err(DispError::Backend)
    }

    /// Set scale with validation.
    pub fn set_scale<B: DispBackend>(be: &B, name: &str, scale: f32) -> Result<(), DispError> {
        Scale::new(scale)?;
        be.set_scale(name, scale).map_err(DispError::Backend)
    }

    /// Arrange outputs in a row/column/grid.
    /// `positions` maps output name → Position.
    pub fn arrange<B: DispBackend>(be: &B,
        positions: &BTreeMap<String, Position>) -> Result<(), DispError> {
        // check for overlapping rectangles (approximate: same position = overlap)
        let mut seen = std::collections::BTreeSet::new();
        for pos in positions.values() {
            if !seen.insert((pos.x, pos.y)) {
                return Err(DispError::Overlap(format!("{},{}", pos.x, pos.y)));
            }
        }
        for (name, pos) in positions {
            be.set_position(name, *pos).map_err(DispError::Backend)?;
        }
        Ok(())
    }

    /// Auto-arrange: place outputs left-to-right starting at 0,0.
    /// Uses current mode width (or preferred) for spacing.
    pub fn auto_arrange<B: DispBackend>(be: &B) -> Result<BTreeMap<String, Position>, DispError> {
        let outputs = Self::connected(be)?;
        let mut pos = BTreeMap::new();
        let mut x = 0i32;
        for o in &outputs {
            pos.insert(o.name.clone(), Position { x, y: 0 });
            let w = o.current.or(o.preferred).map(|m| m.width as i32).unwrap_or(1920);
            x += w;
        }
        // apply
        for (name, p) in &pos {
            be.set_position(name, *p).map_err(DispError::Backend)?;
        }
        Ok(pos)
    }

    /// Mirror mode: set all outputs to same mode.
    pub fn mirror<B: DispBackend>(be: &B, mode: DisplayMode) -> Result<(), DispError> {
        for o in Self::connected(be)? {
            Self::set_mode(be, &o.name, mode)?;
        }
        Ok(())
    }

    /// Render output list.
    pub fn render(outputs: &[Output]) -> String {
        let mut out = String::new();
        out.push_str(&format!("{:<12} {:<6} {:<14} {:<14} {:<8} {:<6} {}\n",
            "NAME","CONN","MODE","POS","SCALE","PRI","MODEL"));
        for o in outputs {
            out.push_str(&format!("{:<12} {:<6} {:<14} {:>5},{:<5} {:<8} {:<6} {}\n",
                o.name, if o.connected {"yes"} else {"no"},
                o.current.map(|m| m.label()).unwrap_or_else(|| "-".into()),
                o.position.x, o.position.y,
                format!("{:.2}", o.scale.0),
                if o.primary {"*"} else {""},
                o.model));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_output(name: &str, w: u32, h: u32) -> Output {
        Output {
            name: name.into(), connected: true,
            width_mm: 640, height_mm: 360,
            preferred: Some(DisplayMode::new(w, h, 60)),
            modes: vec![
                DisplayMode::new(1920, 1080, 60),
                DisplayMode::new(1920, 1080, 144),
                DisplayMode::new(2560, 1440, 60),
                DisplayMode::new(3840, 2160, 30),
            ],
            current: Some(DisplayMode::new(w, h, 60)),
            position: Position::default(), primary: false,
            rotation: Rotation::Normal, scale: Scale(1.0),
            brightness: 1.0,
            manufacturer: "DEL".into(), model: "U2720Q".into(), serial: "CN0".into(),
        }
    }

    fn be() -> FakeBackend {
        let mut be = FakeBackend::new();
        *be.outputs.borrow_mut() = vec![
            sample_output("eDP-1", 1920, 1080),
            sample_output("DP-1", 2560, 1440),
            sample_output("HDMI-A-1", 3840, 2160),
        ];
        // mark one disconnected
        be.outputs.borrow_mut()[2].connected = false;
        be.backlights = vec![("intel_backlight".into(), 937)];
        be
    }

    #[test]
    fn mode_label() {
        let m = DisplayMode::new(1920, 1080, 60);
        assert_eq!(m.label(), "1920x1080@60");
        assert_eq!(m.refresh_mhz, 60000);
    }

    #[test]
    fn connected_filters() {
        let be = be();
        let c = DisplayManager::connected(&be).unwrap();
        assert_eq!(c.len(), 2); // HDMI disconnected
        assert!(c.iter().any(|o| o.name == "eDP-1"));
        assert!(c.iter().any(|o| o.name == "DP-1"));
    }

    #[test]
    fn set_mode_validates() {
        let be = be();
        // valid mode
        DisplayManager::set_mode(&be, "eDP-1", DisplayMode::new(2560,1440,60)).unwrap();
        // invalid mode not in list
        assert!(DisplayManager::set_mode(&be, "eDP-1", DisplayMode::new(800,600,60)).is_err());
        // nonexistent output
        assert!(DisplayManager::set_mode(&be, "DP-99", DisplayMode::new(1920,1080,60)).is_err());
    }

    #[test]
    fn brightness_validation() {
        let be = be();
        DisplayManager::set_brightness(&be, "eDP-1", 0.5).unwrap();
        assert!(DisplayManager::set_brightness(&be, "eDP-1", 1.5).is_err());
        assert!(DisplayManager::set_brightness(&be, "eDP-1", -0.1).is_err());
    }

    #[test]
    fn scale_validation() {
        let be = be();
        DisplayManager::set_scale(&be, "eDP-1", 2.0).unwrap();
        assert!(DisplayManager::set_scale(&be, "eDP-1", 0.1).is_err());
        assert!(DisplayManager::set_scale(&be, "eDP-1", 5.0).is_err());
    }

    #[test]
    fn arrange_detects_overlap() {
        let be = be();
        let mut pos = BTreeMap::new();
        pos.insert("eDP-1".to_string(), Position{x:0,y:0});
        pos.insert("DP-1".to_string(), Position{x:0,y:0}); // same → overlap
        assert!(DisplayManager::arrange(&be, &pos).is_err());
    }

    #[test]
    fn auto_arrange_positions() {
        let be = be();
        let pos = DisplayManager::auto_arrange(&be).unwrap();
        // eDP-1 at 0, DP-1 at 1920 (eDP width)
        assert_eq!(pos.get("eDP-1"), Some(&Position{x:0,y:0}));
        assert_eq!(pos.get("DP-1"), Some(&Position{x:1920,y:0}));
        // HDMI not in connected → not arranged
        assert!(!pos.contains_key("HDMI-A-1"));
    }

    #[test]
    fn mirror_sets_all() {
        let be = be();
        DisplayManager::mirror(&be, DisplayMode::new(1920,1080,60)).unwrap();
        let calls = be.calls.borrow();
        let mode_calls: Vec<_> = calls.iter().filter(|c| c.starts_with("mode")).collect();
        assert_eq!(mode_calls.len(), 2); // only connected outputs
    }

    #[test]
    fn rotation_degrees() {
        assert_eq!(Rotation::Normal.degrees(), 0);
        assert_eq!(Rotation::Right.degrees(), 90);
        assert_eq!(Rotation::parse("left"), Some(Rotation::Left));
        assert_eq!(Rotation::parse("bogus"), None);
    }

    #[test]
    fn laptop_vs_external() {
        assert!(sample_output("eDP-1",1920,1080).is_laptop_panel());
        assert!(sample_output("DP-1",2560,1440).is_external());
        assert!(sample_output("HDMI-A-1",3840,2160).is_external());
    }

    #[test]
    fn aspect_ratio() {
        let o = sample_output("DP-1", 2560, 1440);
        assert!((o.aspect() - 1.778).abs() < 0.01);
    }

    #[test]
    fn render_outputs() {
        let be = be();
        let outs = be.outputs().unwrap();
        let t = DisplayManager::render(&outs);
        assert!(t.contains("NAME"));
        assert!(t.contains("eDP-1"));
        assert!(t.contains("2560x1440@60"));
    }

    #[test]
    fn backlights_listed() {
        let be = be();
        let bl = be.backlights().unwrap();
        assert_eq!(bl.len(), 1);
        assert_eq!(bl[0].0, "intel_backlight");
        assert_eq!(bl[0].1, 937);
    }
}
