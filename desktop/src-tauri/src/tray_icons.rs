//! 托盘使用原版图标蒙版；Windows 的固定图标区域以双行位图显示速率。
use tauri::image::Image;

pub struct TrayIcons {
    running_light: Image<'static>,
    running_dark: Image<'static>,
    stopped_light: Image<'static>,
    stopped_dark: Image<'static>,
}

impl TrayIcons {
    pub fn load() -> tauri::Result<Self> {
        let running = Image::from_bytes(include_bytes!("../icons/run.png"))?;
        let stopped = Image::from_bytes(include_bytes!("../icons/sleep.png"))?;
        Ok(Self {
            running_light: tint(&running, 0),
            running_dark: tint(&running, 255),
            stopped_light: tint(&stopped, 0),
            stopped_dark: tint(&stopped, 255),
        })
    }

    pub fn image(&self, running: bool, light_taskbar: bool) -> Image<'static> {
        match (running, light_taskbar) {
            (true, true) => self.running_light.clone(),
            (true, false) => self.running_dark.clone(),
            (false, true) => self.stopped_light.clone(),
            (false, false) => self.stopped_dark.clone(),
        }
    }

    pub fn with_speed(
        &self,
        running: bool,
        light: bool,
        style: &str,
        speed: Option<(u64, u64)>,
    ) -> Image<'static> {
        if style == "iconOnly" {
            return self.image(running, light);
        }
        let mut pixels = vec![0; 32 * 32 * 4];
        let ink = if light { 0 } else { 255 };
        if style == "iconAndSpeed" {
            let source = self.image(running, light);
            for y in 0..10usize {
                for x in 0..10usize {
                    let sx = x * source.width() as usize / 10;
                    let sy = y * source.height() as usize / 10;
                    let from = (sy * source.width() as usize + sx) * 4;
                    let to = ((y + 1) * 32 + x + 11) * 4;
                    pixels[to..to + 4].copy_from_slice(&source.rgba()[from..from + 4]);
                }
            }
        }
        let top = if style == "iconAndSpeed" { 13 } else { 7 };
        for (row, value) in [speed.map(|s| s.0), speed.map(|s| s.1)]
            .into_iter()
            .enumerate()
        {
            let label = value.map(compact_speed).unwrap_or_else(|| "--".into());
            let text = format!("{}{label}", if row == 0 { '^' } else { 'v' });
            let x = (32usize.saturating_sub(text.len() * 4)) / 2;
            for (index, character) in text.chars().enumerate() {
                draw_glyph(&mut pixels, character, x + index * 4, top + row * 8, ink);
            }
        }
        Image::new_owned(pixels, 32, 32)
    }
}

fn compact_speed(bytes: u64) -> String {
    let (value, unit) = if bytes >= 1_073_741_824 {
        (bytes / 1_073_741_824, 'g')
    } else if bytes >= 1_048_576 {
        (bytes / 1_048_576, 'm')
    } else if bytes >= 1024 {
        (bytes / 1024, 'k')
    } else {
        (bytes, 'b')
    };
    format!("{}{unit}", value.min(999))
}

fn draw_glyph(pixels: &mut [u8], character: char, x: usize, y: usize, ink: u8) {
    let rows = match character {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'k' => [5, 6, 4, 6, 5],
        'm' => [0, 7, 7, 5, 5],
        'g' => [7, 5, 7, 1, 7],
        'b' => [4, 4, 6, 5, 6],
        '^' => [2, 7, 2, 2, 0],
        'v' => [0, 2, 2, 7, 2],
        '-' => [0, 0, 7, 0, 0],
        _ => [0; 5],
    };
    for (dy, bits) in rows.into_iter().enumerate() {
        for dx in 0..3 {
            if bits & (1 << (2 - dx)) != 0 && x + dx < 32 && y + dy < 32 {
                let offset = ((y + dy) * 32 + x + dx) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[ink, ink, ink, 255]);
            }
        }
    }
}

fn tint(source: &Image<'_>, ink: u8) -> Image<'static> {
    let mut rgba = source.rgba().to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[..3].fill(ink);
    }
    Image::new_owned(rgba, source.width(), source.height())
}

pub fn light_taskbar() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
        };
        let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let name: Vec<u16> = "SystemUsesLightTheme"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // Read only the current user's system/taskbar preference, not the separate app theme.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        result == ERROR_SUCCESS && size == 4 && value != 0
    }
    #[cfg(not(windows))]
    false
}
