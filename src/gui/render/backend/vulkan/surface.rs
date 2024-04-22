use ash::{extensions::khr, vk};
use raw_window_handle::HasRawWindowHandle;

pub struct Surface {
    pub raw: vk::SurfaceKHR,
    pub fns: khr::Surface,
}

impl Surface {
    pub fn create(
        instance: &super::Instance,
        window: &impl HasRawWindowHandle,
    ) -> anyhow::Result<Self> {
        let surface =
            unsafe { ash_window::create_surface(&instance.entry, &instance.raw, window, None)? };
        let surface_loader = khr::Surface::new(&instance.entry, &instance.raw);

        Ok(Self {
            raw: surface,
            fns: surface_loader,
        })
    }
}
