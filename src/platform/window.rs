use anyhow::Context;
#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowLevel};

use crate::core::pet::Rect;

#[derive(Debug, Clone, Copy)]
pub enum WindowCommand {
    MoveTo { x: f64, y: f64 },
}

pub struct PetWindow {
    window: Window,
}

impl PetWindow {
    pub fn new(event_loop: &ActiveEventLoop) -> anyhow::Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title("Pet Blackhole")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(560.0, 560.0));

        let window = event_loop
            .create_window(attrs)
            .context("create winit window")?;
        window.set_cursor_visible(true);

        if let Some(monitor) = window
            .current_monitor()
            .or_else(|| event_loop.primary_monitor())
        {
            let scale = monitor.scale_factor();
            let position = monitor.position();
            let size = monitor.size();
            let x = f64::from(position.x) / scale + f64::from(size.width) / scale - 620.0;
            let y = f64::from(position.y) / scale + f64::from(size.height) / scale - 620.0;
            window.set_outer_position(LogicalPosition::new(x, y));
        }

        Ok(Self { window })
    }

    pub fn raw(&self) -> &Window {
        &self.window
    }

    pub fn drag_to_cursor(&self, cursor: (f32, f32), drag_offset: (f32, f32)) {
        let (x, y) = self.outer_position();
        let new_x = x + f64::from(cursor.0 - drag_offset.0);
        let new_y = y + f64::from(cursor.1 - drag_offset.1);
        self.window
            .set_outer_position(LogicalPosition::new(new_x, new_y));
    }

    pub fn set_outer_position(&self, position: LogicalPosition<f64>) {
        self.window.set_outer_position(position);
    }

    pub fn apply_hit_region(&self, points: &[[f32; 2]]) {
        #[cfg(windows)]
        self.apply_windows_hit_region(points);
        #[cfg(not(windows))]
        let _ = points;
    }

    #[cfg(windows)]
    fn apply_windows_hit_region(&self, points: &[[f32; 2]]) {
        use windows::Win32::Foundation::{HWND, POINT};
        use windows::Win32::Graphics::Gdi::{CreatePolygonRgn, SetWindowRgn, WINDING};

        let Ok(handle) = self.window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(win32.hwnd.get() as *mut _);
        let size = self.window.inner_size();
        let w = size.width.max(1) as f32;
        let h = size.height.max(1) as f32;

        let center = points
            .iter()
            .fold([0.0_f32, 0.0_f32], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
        let center = [
            center[0] / points.len() as f32,
            center[1] / points.len() as f32,
        ];
        let polygon: Vec<POINT> = points
            .iter()
            .map(|p| {
                let x = center[0] + (p[0] - center[0]) * 1.18;
                let y = center[1] + (p[1] - center[1]) * 1.18;
                POINT {
                    x: (x.clamp(0.0, 1.0) * w).round() as i32,
                    y: (y.clamp(0.0, 1.0) * h).round() as i32,
                }
            })
            .collect();

        unsafe {
            let region = CreatePolygonRgn(&polygon, WINDING);
            if !region.is_invalid() && SetWindowRgn(hwnd, Some(region), true) == 0 {
                log::debug!("SetWindowRgn failed");
            }
        }
    }

    pub fn outer_position(&self) -> (f64, f64) {
        self.window
            .outer_position()
            .map(physical_position_to_logical_tuple(&self.window))
            .unwrap_or((0.0, 0.0))
    }

    pub fn outer_size(&self) -> (f64, f64) {
        physical_size_to_logical_tuple(&self.window)(self.window.outer_size())
    }

    pub fn monitor_work_area(&self) -> Rect {
        let Some(monitor) = self.window.current_monitor() else {
            return Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            };
        };
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        Rect {
            x: f64::from(position.x) / scale,
            y: f64::from(position.y) / scale,
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
        }
    }
}

fn physical_position_to_logical_tuple(
    window: &Window,
) -> impl FnOnce(PhysicalPosition<i32>) -> (f64, f64) + '_ {
    move |position| {
        let logical: LogicalPosition<f64> = position.to_logical(window.scale_factor());
        (logical.x, logical.y)
    }
}

fn physical_size_to_logical_tuple(
    window: &Window,
) -> impl FnOnce(PhysicalSize<u32>) -> (f64, f64) + '_ {
    move |size| {
        let logical: LogicalSize<f64> = size.to_logical(window.scale_factor());
        (logical.width, logical.height)
    }
}
