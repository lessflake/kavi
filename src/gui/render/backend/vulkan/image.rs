use std::sync::Arc;

use super::Device;
use ash::vk;

pub struct Image {
    pub raw: vk::Image,
    pub memory: Option<vk::DeviceMemory>,

    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub usage: vk::ImageUsageFlags,

    pub device: Arc<Device>,
}

pub struct ImageView {
    pub raw: vk::ImageView,
    pub device: Arc<Device>,
    // pub image: Arc<Image>,
}

impl Image {
    pub fn view(&self, format: vk::Format) -> anyhow::Result<ImageView> {
        let subresource_range = vk::ImageSubresourceRange::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(1)
            .base_array_layer(0)
            .layer_count(1)
            .build();

        let view_info = vk::ImageViewCreateInfo::builder()
            .image(self.raw)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .components(
                vk::ComponentMapping::builder()
                    .r(vk::ComponentSwizzle::IDENTITY)
                    .g(vk::ComponentSwizzle::IDENTITY)
                    .b(vk::ComponentSwizzle::IDENTITY)
                    .a(vk::ComponentSwizzle::IDENTITY)
                    .build(),
            )
            .subresource_range(subresource_range);

        let view = unsafe { self.device.raw.create_image_view(&view_info, None) }?;

        Ok(ImageView {
            raw: view,
            device: Arc::clone(&self.device),
            // image: Arc::clone(self),
        })
    }
}

impl Device {
    pub fn create_image(
        self: &Arc<Self>,
        format: vk::Format,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
        memory_properties: vk::MemoryPropertyFlags,
    ) -> anyhow::Result<Image> {
        let queue_family_indices = [self.queue.family];
        let image_info = vk::ImageCreateInfo::builder()
            // .flags(vk::ImageCreateFlags::PROTECTED)
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(
                vk::Extent3D::builder()
                    .width(extent.width)
                    .height(extent.height)
                    .depth(1)
                    .build(),
            )
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL) // TODO: would like this to be linear?
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .queue_family_indices(&queue_family_indices)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let (image, memory) = unsafe {
            let image = self.raw.create_image(&image_info, None)?;
            let memory_requirements = self.raw.get_image_memory_requirements(image);
            let memory = self.allocate_memory(memory_requirements, memory_properties)?;
            self.raw.bind_image_memory(image, memory, 0)?;
            (image, memory)
        };

        Ok(Image {
            raw: image,
            memory: Some(memory),
            format,
            extent,
            usage,
            device: Arc::clone(self),
        })
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            if let Some(memory) = self.memory {
                self.device.raw.destroy_image(self.raw, None);
                self.device.raw.free_memory(memory, None);
            }
        }
    }
}

impl Drop for ImageView {
    fn drop(&mut self) {
        unsafe { self.device.raw.destroy_image_view(self.raw, None) }
    }
}
