use super::{Instance, Surface};
use ash::vk;

pub struct PhysicalDevice {
    pub raw: vk::PhysicalDevice,
    pub properties: vk::PhysicalDeviceProperties,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub queue_families: Vec<QueueFamily>,
}

#[derive(Clone)]
pub struct QueueFamily {
    pub index: u32,
    pub properties: vk::QueueFamilyProperties,
}

pub fn enumerate_physical_devices(
    instance: &Instance,
) -> anyhow::Result<impl Iterator<Item = PhysicalDevice> + '_> {
    unsafe {
        let pdevices = instance.raw.enumerate_physical_devices()?;

        Ok(EnumeratePhysicalDevices {
            instance,
            inner: pdevices.into_iter(),
        })
    }
}

struct EnumeratePhysicalDevices<'a, T>
where
    T: Iterator<Item = vk::PhysicalDevice>,
{
    instance: &'a Instance,
    inner: T,
}

impl<'a, T> Iterator for EnumeratePhysicalDevices<'a, T>
where
    T: Iterator<Item = vk::PhysicalDevice>,
{
    type Item = PhysicalDevice;

    fn next(&mut self) -> Option<Self::Item> {
        let pdevice = self.inner.next()?;

        unsafe {
            let properties = self.instance.raw.get_physical_device_properties(pdevice);
            let memory_properties = self
                .instance
                .raw
                .get_physical_device_memory_properties(pdevice);
            let queue_families = self
                .instance
                .raw
                .get_physical_device_queue_family_properties(pdevice)
                .into_iter()
                .enumerate()
                .map(|(index, properties)| QueueFamily {
                    index: index as _,
                    properties,
                })
                .collect();

            Some(PhysicalDevice {
                raw: pdevice,
                properties,
                memory_properties,
                queue_families,
            })
        }
    }
}

pub trait PhysicalDeviceIterExt {
    fn with_presentation_support(self, surface: &Surface) -> PresentationSupport<Self>
    where
        Self: Sized;
}

pub struct PresentationSupport<'b, T> {
    inner: T,
    surface: &'b Surface,
}

impl<'a, 'b, T> Iterator for PresentationSupport<'b, T>
where
    T: Iterator<Item = PhysicalDevice> + 'a,
{
    type Item = PhysicalDevice;

    fn next(&mut self) -> Option<Self::Item> {
        let pdevice = self.inner.next()?;

        let supports_presentation =
            pdevice
                .queue_families
                .iter()
                .enumerate()
                .any(|(queue_index, info)| unsafe {
                    info.properties
                        .queue_flags
                        .contains(vk::QueueFlags::GRAPHICS)
                        && self
                            .surface
                            .fns
                            .get_physical_device_surface_support(
                                pdevice.raw,
                                queue_index as u32,
                                self.surface.raw,
                            )
                            .unwrap()
                });

        if supports_presentation {
            Some(pdevice)
        } else {
            None
        }
    }
}

impl<T> PhysicalDeviceIterExt for T
where
    T: Iterator<Item = PhysicalDevice>,
{
    fn with_presentation_support(self, surface: &Surface) -> PresentationSupport<'_, Self>
    where
        Self: Sized,
    {
        PresentationSupport {
            inner: self,
            surface,
        }
    }
}
