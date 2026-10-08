//! The source macOS icons are template masks; Windows needs explicit ink colors.
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
