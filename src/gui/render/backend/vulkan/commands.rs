use super::{pipeline::Pipeline, Buffer, Device, Image};
use ash::vk;
use std::sync::Arc;

pub struct CommandBuffer {
    pub raw: vk::CommandBuffer,
    pub device: Arc<Device>,
}

#[repr(transparent)]
pub struct RecordingCommandBuffer<'a>(pub &'a CommandBuffer);

impl CommandBuffer {
    pub fn record(&self, callback: impl FnOnce(RecordingCommandBuffer)) -> anyhow::Result<()> {
        let begin_info = vk::CommandBufferBeginInfo::builder();
        unsafe {
            self.device
                .raw
                .begin_command_buffer(self.raw, &begin_info)?;
        }

        callback(RecordingCommandBuffer(self));

        unsafe {
            self.device.raw.end_command_buffer(self.raw)?;
        }

        Ok(())
    }
}

impl RecordingCommandBuffer<'_> {
    pub fn bind_pipeline<P>(&self, pipeline: &P)
    where
        P: Pipeline,
    {
        unsafe {
            self.0.device.raw.cmd_bind_pipeline(
                self.0.raw,
                P::BIND_POINT,
                pipeline.common().pipeline,
            );
        }
    }

    pub fn bind_descriptor_set<P>(&self, pipeline: &P, descriptor_set: &super::DescriptorSet)
    where
        P: Pipeline,
    {
        unsafe {
            self.0.device.raw.cmd_bind_descriptor_sets(
                self.0.raw,
                P::BIND_POINT,
                pipeline.common().pipeline_layout,
                0,
                std::slice::from_ref(&descriptor_set.raw),
                &[],
            );
        }
    }

    pub fn dispatch(&self, x: u32, y: u32, z: u32) {
        unsafe {
            self.0.device.raw.cmd_dispatch(self.0.raw, x, y, z);
        }
    }

    pub fn with_render_pass(
        &self,
        render_pass: &super::pipeline::RenderPass,
        framebuffer: &super::Framebuffer,
        callback: impl FnOnce(CommandBufferInRenderPass<'_>),
    ) {
        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.3, 0.7, 1.0],
            },
        }];

        let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
            .render_pass(render_pass.raw)
            .framebuffer(framebuffer.raw)
            .render_area(
                vk::Rect2D::builder()
                    .offset(vk::Offset2D::builder().x(0).y(0).build())
                    .extent(framebuffer.image.extent)
                    .build(),
            )
            .clear_values(&clear_values);

        unsafe {
            self.0.device.raw.cmd_begin_render_pass(
                self.0.raw,
                &render_pass_begin_info,
                vk::SubpassContents::INLINE,
            );
        }

        callback(CommandBufferInRenderPass(self));

        unsafe {
            self.0.device.raw.cmd_end_render_pass(self.0.raw);
        }
    }

    pub fn image_barrier(
        &self,
        image: &Image,
        src_access: vk::AccessFlags,
        dst_access: vk::AccessFlags,
        old_layout: vk::ImageLayout,
        new_layout: vk::ImageLayout,
        src_stage: vk::PipelineStageFlags,
        dst_stage: vk::PipelineStageFlags,
    ) {
        let subresource_range = vk::ImageSubresourceRange::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(1)
            .base_array_layer(0)
            .layer_count(1)
            .build();

        let image_memory_barrier = vk::ImageMemoryBarrier::builder()
            // .src_access_mask(vk::AccessFlags::empty())
            // .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
            // .old_layout(vk::ImageLayout::UNDEFINED)
            // .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(src_access)
            .dst_access_mask(dst_access)
            .old_layout(old_layout)
            .new_layout(new_layout)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image.raw)
            .subresource_range(subresource_range)
            .build();

        unsafe {
            self.0.device.raw.cmd_pipeline_barrier(
                self.0.raw,
                // vk::PipelineStageFlags::TOP_OF_PIPE,
                // vk::PipelineStageFlags::COMPUTE_SHADER,
                src_stage,
                dst_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_memory_barrier],
            );
        }
    }

    pub fn push_constants<P>(&self, pipeline: &P, value: &[u8])
    where
        P: Pipeline,
    {
        unsafe {
            self.0.device.raw.cmd_push_constants(
                self.0.raw,
                pipeline.common().pipeline_layout,
                P::SHADER_STAGE,
                0,
                value,
            );
        }
    }

    pub fn copy_buffer_to_image(&self, buffer: &Buffer, image: &Image) {
        let subresource_range = vk::ImageSubresourceLayers::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(0)
            .base_array_layer(0)
            .layer_count(1)
            .build();

        let region = vk::BufferImageCopy::builder()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(subresource_range)
            .image_offset(vk::Offset3D::builder().x(0).y(0).z(0).build())
            .image_extent(
                vk::Extent3D::builder()
                    .width(image.extent.width)
                    .height(image.extent.height)
                    .depth(1)
                    .build(),
            );

        unsafe {
            self.0.device.raw.cmd_copy_buffer_to_image(
                self.0.raw,
                buffer.raw,
                image.raw,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&region),
            );
        }
    }
}

#[repr(transparent)]
pub struct CommandBufferInRenderPass<'a>(&'a RecordingCommandBuffer<'a>);

impl<'a> CommandBufferInRenderPass<'a> {
    pub fn draw(&self, vertex_count: u32, first_vertex: u32) {
        unsafe {
            self.0
                 .0
                .device
                .raw
                .cmd_draw(self.0 .0.raw, vertex_count, 1, first_vertex, 0);
        }
    }
}

impl<'a> std::ops::Deref for CommandBufferInRenderPass<'a> {
    type Target = RecordingCommandBuffer<'a>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
