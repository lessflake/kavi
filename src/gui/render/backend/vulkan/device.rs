use super::{physical_device::PhysicalDevice, CommandBuffer, Instance};
use anyhow::Context as _;
use ash::vk;
use std::{ffi::CStr, sync::Arc};

pub struct Device {
    pub raw: ash::Device,
    pub physical_device: PhysicalDevice,
    pub instance: Arc<Instance>,
    pub queue: Queue,
    pub command_pool: vk::CommandPool,
}

pub struct Queue {
    pub raw: vk::Queue,
    pub family: u32,
}

impl Device {
    pub fn create(
        instance: Arc<Instance>,
        pdevice: PhysicalDevice,
        validation_layers: bool,
    ) -> anyhow::Result<Arc<Self>> {
        let extensions = vec![
            vk::KhrSwapchainFn::name().as_ptr(),
            vk::KhrExternalMemoryFn::name().as_ptr(),
            vk::KhrExternalMemoryWin32Fn::name().as_ptr(),
        ];
        let mut layers = vec![];

        if validation_layers {
            layers.push(super::LAYER_KHRONOS_VALIDATION_LAYER_NAME);
        }

        unsafe {
            let extension_properties = instance
                .raw
                .enumerate_device_extension_properties(pdevice.raw)?;

            for &ext in extensions.iter() {
                let ext = CStr::from_ptr(ext);
                if !extension_properties
                    .iter()
                    .map(|properties| CStr::from_ptr(properties.extension_name.as_ptr()))
                    .any(|supported_ext| ext == supported_ext)
                {
                    anyhow::bail!("device extension not supported: {:?}", ext);
                }
            }
        }

        let queue_family = pdevice
            .queue_families
            .iter()
            .filter(|qf| qf.properties.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .cloned()
            .next()
            .context("no suitable render queue found")?;

        let queue_infos = [vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family.index)
            .queue_priorities(&[1.0])
            .build()];

        let features =
            vk::PhysicalDeviceFeatures::builder().shader_storage_image_extended_formats(true);
        let mut v12_features = vk::PhysicalDeviceVulkan12Features::builder()
            .vulkan_memory_model(true)
            .timeline_semaphore(true);

        let device_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&queue_infos)
            .enabled_features(&features)
            .enabled_extension_names(&extensions)
            .enabled_layer_names(&layers)
            .push_next(&mut v12_features)
            .build();

        let device = unsafe {
            instance
                .raw
                .create_device(pdevice.raw, &device_info, None)?
        };

        let queue = Queue {
            raw: unsafe { device.get_device_queue(queue_family.index, 0) },
            family: queue_family.index,
        };

        let info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue.family);
        let command_pool = unsafe { device.create_command_pool(&info, None) }?;

        Ok(Arc::new(Self {
            raw: device,
            physical_device: pdevice,
            instance,
            queue,
            command_pool,
        }))
    }

    pub fn find_memory_type_index(
        &self,
        bits: u32,
        flags: ash::vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        unsafe {
            self.instance
                .raw
                .get_physical_device_memory_properties(self.physical_device.raw)
                .memory_types
                .iter()
                .enumerate()
                .position(|(i, mem_type)| {
                    ((bits & (1 << i)) != 0) && ((mem_type.property_flags & flags) == flags)
                })
                .map(|idx| idx as u32)
        }
    }

    pub fn allocate_memory(
        &self,
        requirements: vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
    ) -> anyhow::Result<vk::DeviceMemory> {
        let bits = requirements.memory_type_bits;
        let memory_type = self
            .find_memory_type_index(bits, flags)
            .context("failed to find suitable memory type")?;

        let info = ash::vk::MemoryAllocateInfo::builder()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type as u32);

        Ok(unsafe { self.raw.allocate_memory(&info, None) }?)
    }

    // pub fn create_command_pool(self: &Arc<Device>) -> anyhow::Result<CommandPool> {
    //     let info = vk::CommandPoolCreateInfo::builder()
    //         .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
    //         .queue_family_index(self.queue.family);
    //     let command_pool = unsafe { self.raw.create_command_pool(&info, None) }?;

    //     Ok(CommandPool {
    //         raw: command_pool,
    //         device: Arc::clone(self),
    //     })
    // }

    pub fn wait_for_fence(self: &Device, fence: vk::Fence) -> anyhow::Result<()> {
        Ok(unsafe {
            self.raw
                .wait_for_fences(std::slice::from_ref(&fence), true, u64::MAX)
        }?)
    }

    pub fn one_time_submit(
        self: &Arc<Self>,
        callback: impl FnOnce(super::commands::RecordingCommandBuffer<'_>),
    ) -> anyhow::Result<()> {
        let buffer_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(self.command_pool)
            .command_buffer_count(1)
            .level(vk::CommandBufferLevel::PRIMARY);
        let buffer = unsafe { self.raw.allocate_command_buffers(&buffer_info) }?;

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.raw.begin_command_buffer(buffer[0], &begin_info)?;
        }

        let buffer = CommandBuffer {
            raw: buffer[0],
            device: Arc::clone(self),
        };

        callback(super::commands::RecordingCommandBuffer(&buffer));

        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(&[buffer.raw])
            .build();

        unsafe {
            self.raw.end_command_buffer(buffer.raw)?;
            self.raw
                .queue_submit(self.queue.raw, &[submit_info], vk::Fence::null())?;
            self.raw.queue_wait_idle(self.queue.raw)?;
            self.raw
                .free_command_buffers(self.command_pool, &[buffer.raw]);
        }

        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            self.raw.destroy_command_pool(self.command_pool, None);
            self.raw.destroy_device(None);
        }
    }
}

pub struct CommandPool {
    pub raw: vk::CommandPool,
    device: Arc<Device>,
}

impl CommandPool {}

impl Drop for CommandPool {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.destroy_command_pool(self.raw, None);
        }
    }
}
