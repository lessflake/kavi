use std::sync::Arc;

use super::Device;
use ash::vk;

pub struct Buffer {
    pub raw: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub usage: vk::BufferUsageFlags,
    pub size: u64,
    pub device: Arc<Device>,
}

impl Device {
    pub fn create_buffer(
        self: &Arc<Self>,
        usage: vk::BufferUsageFlags,
        memory_flags: vk::MemoryPropertyFlags,
        size: u64,
    ) -> anyhow::Result<Buffer> {
        let info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let (buffer, memory) = unsafe {
            let buffer = self.raw.create_buffer(&info, None)?;
            let requirements = self.raw.get_buffer_memory_requirements(buffer);
            let memory = self.allocate_memory(requirements, memory_flags)?;
            self.raw.bind_buffer_memory(buffer, memory, 0)?;
            (buffer, memory)
        };

        Ok(Buffer {
            raw: buffer,
            memory,
            usage,
            size,
            device: Arc::clone(self),
        })
    }
}

impl Buffer {
    pub fn map_memory<'a, T>(
        &'a mut self,
        start: usize,
        count: usize,
    ) -> anyhow::Result<&'a mut [T]> {
        let start = start as u64;
        let count = count as u64;
        let size_of_t = std::mem::size_of::<T>() as u64;
        anyhow::ensure!(
            ((start + count) * size_of_t) <= self.size,
            "attempt to map memory outside of buffer limits"
        );
        let mapped_ptr = unsafe {
            self.device.raw.map_memory(
                self.memory,
                size_of_t * start,
                size_of_t * count,
                vk::MemoryMapFlags::empty(),
            )
        }? as *mut T;
        let data = unsafe { std::slice::from_raw_parts_mut(mapped_ptr, count as usize) };
        Ok(data)
    }

    pub fn unmap_memory(&self) {
        unsafe {
            self.device.raw.unmap_memory(self.memory);
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.free_memory(self.memory, None);
            self.device.raw.destroy_buffer(self.raw, None);
        }
    }
}
