use super::{Buffer, Device, Image, ImageView};
use ash::vk;
use std::sync::Arc;

pub struct DescriptorPool {
    pub(crate) raw: ash::vk::DescriptorPool,
    pub(crate) device: Arc<Device>,
}

impl Drop for DescriptorPool {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.destroy_descriptor_pool(self.raw, None);
        }
    }
}

pub struct DescriptorSet {
    pub raw: vk::DescriptorSet,
    // pub layout: vk::DescriptorSetLayout,
    pub pool: Arc<DescriptorPool>,
}

macro_rules! _update {
    ($ds:expr$(,$b:expr;$a:expr => $d:expr)+ $(,)?) => {
        unsafe {
            $ds.pool.device
                .raw
                .update_descriptor_sets(&[
                    $(
                    $ds.write_descriptors($b,$a as u32,&$d).raw
                    ,)+
                ], &[]);
        };

    };
}
pub(crate) use _update as update;

impl DescriptorSet {
    pub fn write_buffer(&self, binding: u32, array_element: u32, buffer: &Buffer) {
        let info = vk::DescriptorBufferInfo::builder()
            .buffer(buffer.raw)
            .offset(0)
            .range(vk::WHOLE_SIZE);

        let write = vk::WriteDescriptorSet::builder()
            .dst_set(self.raw)
            .dst_binding(binding)
            .dst_array_element(array_element)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(std::slice::from_ref(&info));

        unsafe {
            self.pool
                .device
                .raw
                .update_descriptor_sets(std::slice::from_ref(&write), &[]);
        }
    }

    pub fn write_descriptors<D>(
        &self,
        binding: u32,
        array_element: u32,
        descriptor: &D,
    ) -> DescriptorWrite<{ D::N }>
    where
        D: Descriptor,
    {
        let descriptor_write = vk::WriteDescriptorSet::builder()
            .dst_set(self.raw)
            .dst_binding(binding)
            .dst_array_element(array_element)
            .descriptor_type(D::kind());

        let info = descriptor.info();

        let mut write = DescriptorWrite {
            raw: descriptor_write.build(),
            info,
        };

        write.raw.descriptor_count = D::N as u32;
        match write.info {
            DescriptorInfo::Buffer(ref info) => write.raw.p_buffer_info = info as _,
            DescriptorInfo::Image(ref info) => write.raw.p_image_info = info as _,
        }

        write
    }
}

pub struct StorageImage {
    image: Image,
    view: ImageView,
}

pub fn create_storage_image(
    device: &Arc<Device>,
    format: vk::Format,
    width: u32,
    height: u32,
) -> anyhow::Result<StorageImage> {
    let storage_image = {
        let image = device.create_image(
            format,
            vk::Extent2D::builder().width(width).height(height).build(),
            vk::ImageUsageFlags::STORAGE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        let view = image.view(image.format)?;

        StorageImage { image, view }
    };

    let subresource_range = vk::ImageSubresourceRange::builder()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .base_mip_level(0)
        .level_count(1)
        .base_array_layer(0)
        .layer_count(1)
        .build();

    let image_memory_barrier = vk::ImageMemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(storage_image.image.raw)
        .subresource_range(subresource_range)
        .build();

    device.one_time_submit(|cb| unsafe {
        device.raw.cmd_pipeline_barrier(
            cb.0.raw,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_memory_barrier],
        );
    })?;

    Ok(storage_image)
}

pub trait ImageDescriptor {
    fn kind() -> vk::DescriptorType;
    fn info(&self) -> vk::DescriptorImageInfo;
}

pub trait Descriptor {
    const N: usize;
    fn kind() -> vk::DescriptorType;
    fn info(&self) -> DescriptorInfo<{ Self::N }>;
}

#[allow(dead_code)]
pub enum DescriptorInfo<const N: usize> {
    Buffer([vk::DescriptorBufferInfo; N]),
    Image([vk::DescriptorImageInfo; N]),
}

impl ImageDescriptor for StorageImage {
    fn kind() -> vk::DescriptorType {
        vk::DescriptorType::STORAGE_IMAGE
    }

    fn info(&self) -> vk::DescriptorImageInfo {
        vk::DescriptorImageInfo::builder()
            .image_layout(vk::ImageLayout::GENERAL)
            .image_view(self.view.raw)
            .build()
    }
}

impl ImageDescriptor for super::Framebuffer {
    fn kind() -> vk::DescriptorType {
        vk::DescriptorType::STORAGE_IMAGE
    }

    fn info(&self) -> vk::DescriptorImageInfo {
        vk::DescriptorImageInfo::builder()
            .image_layout(vk::ImageLayout::GENERAL)
            .image_view(self.view.raw)
            .build()
    }
}

impl<T> Descriptor for T
where
    T: ImageDescriptor,
{
    const N: usize = 1;

    fn kind() -> vk::DescriptorType {
        T::kind()
    }

    fn info(&self) -> DescriptorInfo<{ Self::N }> {
        DescriptorInfo::Image([self.info(); Self::N])
    }
}

impl<const N: usize, T> Descriptor for [T; N]
where
    T: ImageDescriptor,
{
    const N: usize = N;

    fn kind() -> vk::DescriptorType {
        T::kind()
    }

    fn info(&self) -> DescriptorInfo<{ Self::N }> {
        let mut array = [unsafe { std::mem::zeroed() }; Self::N];
        for (i, descriptor) in self.iter().enumerate() {
            array[i] = descriptor.info();
        }
        DescriptorInfo::Image(array)
    }
}

pub struct DescriptorWrite<const N: usize> {
    pub raw: vk::WriteDescriptorSet,
    info: DescriptorInfo<N>,
}

// impl Drop for DescriptorSet {
//     fn drop(&mut self) {
//         unsafe {
//             self.pool
//                 .device
//                 .raw
//                 .destroy_descriptor_set_layout(self.layout, None);
//         }
//     }
// }
