#![allow(unreachable_code)]
mod vulkan;

use anyhow::Context;
use ash::vk;
use raw_window_handle::HasRawWindowHandle;
use std::sync::Arc;
use vulkan::physical_device::PhysicalDeviceIterExt as _;

const FRAMES_IN_FLIGHT: usize = 1;

pub struct Frame {
    pub fb: vulkan::Framebuffer,
    pub cb: vulkan::CommandBuffer,
    in_flight: Option<vk::Fence>,
}

#[allow(dead_code)]
pub struct RenderBackend {
    instance: Arc<vulkan::Instance>,
    surface: vulkan::Surface,
    device: Arc<vulkan::Device>,
    // swapchain: vulkan::Swapchain,
    swapchain: vulkan::DxgiSwapchain,
    render_pass: vulkan::pipeline::RenderPass,

    in_flight_fences: Vec<vk::Fence>,
    frames: Vec<Frame>,
    frame: usize,
    pub profiler: vulkan::profiling::ProfilerData,

    shaders: Vec<&'static vulkan::pipeline::ShaderMetadata>,
    descriptor_set_layouts: Vec<vk::DescriptorSetLayout>,
    compute_pipelines: Vec<ComputePipeline>,
    graphics_pipelines: Vec<GraphicsPipeline>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct ShaderHandle(usize);

impl RenderBackend {
    pub fn new(window: &impl HasRawWindowHandle) -> anyhow::Result<Self> {
        #[cfg(debug)]
        let debug = true;
        #[cfg(not(debug))]
        let debug = false;

        let instance = vulkan::Instance::builder()
            .extensions(ash_window::enumerate_required_extensions(window)?)
            .debug(debug)
            .build()?;
        let surface = vulkan::Surface::create(&instance, window)?;
        let pdevice = vulkan::physical_device::enumerate_physical_devices(&instance)?
            .with_presentation_support(&surface)
            .next()
            .context("no suitable device found")?;

        let device = vulkan::Device::create(instance.clone(), pdevice, debug)?;
        // let swapchain = vulkan::Swapchain::new(&instance, &device, &surface)?;

        let render_pass =
            vulkan::pipeline::create_render_pass(&device, vk::Format::B8G8R8A8_UNORM)?;

        // let framebuffers = swapchain.framebuffers(&device, &render_pass)?;
        let mut swapchain = vulkan::dxgi_swapchain(&instance, &device, &window, &surface)?;
        let framebuffers = swapchain.create_images(&device, &render_pass)?;

        let command_buffer_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(device.command_pool)
            // .command_buffer_count(swapchain.acquire_semaphores.len() as u32)
            .command_buffer_count(swapchain.image_count as u32)
            .level(vk::CommandBufferLevel::PRIMARY);

        let command_buffers = unsafe { device.raw.allocate_command_buffers(&command_buffer_info) }?;

        let frames: Vec<_> = framebuffers
            .into_iter()
            .zip(command_buffers.into_iter())
            .map(|(fb, cb)| Frame {
                fb,
                cb: vulkan::CommandBuffer {
                    raw: cb,
                    device: Arc::clone(&device),
                },
                in_flight: None,
            })
            .collect();

        let in_flight_fences: Vec<_> = (0..FRAMES_IN_FLIGHT)
            .map(|_| {
                let fence_info =
                    vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
                Ok(unsafe { device.raw.create_fence(&fence_info, None) }?)
            })
            .collect::<anyhow::Result<_>>()?;

        let profiler: vulkan::profiling::ProfilerData =
            vulkan::profiling::ProfilerData::new(&device)?;

        Ok(Self {
            instance,
            surface,
            device,
            swapchain,
            render_pass,

            in_flight_fences,
            frames,
            frame: 0,
            profiler,

            shaders: Vec::new(),
            descriptor_set_layouts: Vec::new(),
            compute_pipelines: Vec::new(),
            graphics_pipelines: Vec::new(),
        })
    }

    pub fn recreate_swapchain(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        unsafe { self.device.raw.device_wait_idle() }?;
        // Things that need to be recreated:
        // - Swapchain
        // - Framebuffers
        // - Pipelines

        // - Not render pass
        // - Command buffers must be re-recorded before submit
        // - Pretty sure not descriptor sets

        // self.swapchain = self
        //     .swapchain
        //     .recreate(&self.surface, vk::Extent2D { width, height })?;
        // let framebuffers = self
        //     .swapchain
        //     .framebuffers(&self.device, &self.render_pass)?;
        // self.frames
        let old_frames: Vec<_> = self.frames.drain(..).map(|f| (f.cb, f.in_flight)).collect();
        self.swapchain.recreate(vk::Extent2D { width, height });
        let framebuffers = self
            .swapchain
            .create_images(&self.device, &self.render_pass)?;

        for i in 0..self.graphics_pipelines.len() {
            let vs = self.graphics_pipelines[i].common.shaders[0].0;
            let fs = self.graphics_pipelines[i].common.shaders[1].0;

            let pipeline = vulkan::pipeline::create_graphics_pipeline(
                &self.device,
                &[vs, fs],
                &self.render_pass,
                &self.descriptor_set_layouts,
                self.swapchain.extent,
                self.graphics_pipelines[i].common.push_constant_bytes,
            )?;

            self.graphics_pipelines[i] = pipeline;
        }

        // let old_frames = std::mem::take(&mut self.frames);
        // self.frames = std::iter::zip(framebuffers, old_frames)
        //     .map(|(fb, Frame { cb, in_flight, .. })| Frame { fb, cb, in_flight })
        //     .collect();
        self.frames = std::iter::zip(framebuffers, old_frames)
            .map(|(fb, (cb, in_flight))| Frame { fb, cb, in_flight })
            .collect();

        Ok(())
    }

    pub fn begin_frame(
        &mut self,
    ) -> anyhow::Result<(
        &Frame,
        vulkan::SwapchainImage,
        &vulkan::profiling::ProfilerData,
    )> {
        let fence = self.in_flight_fences[self.frame];

        self.device.wait_for_fence(fence)?;

        let next_image = match self.swapchain.acquire_next_image() {
            Ok(img) => img,
            Err(_) => {
                panic!();
                // self.recreate_swapchain()?;
                // self.swapchain
                // .acquire_next_image()
                // .unwrap()
            }
        };

        let frame = &mut self.frames[next_image.index as usize];

        if let Some(fence) = frame.in_flight {
            self.device.wait_for_fence(fence)?;
        }
        frame.in_flight = Some(fence);

        Ok((frame, next_image, &self.profiler))
    }

    pub fn draw_frame(&mut self, next_image: vulkan::SwapchainImage) -> anyhow::Result<()> {
        let frame = &mut self.frames[next_image.index as usize];
        let fence = self.in_flight_fences[self.frame];
        let raw_device = &self.device.raw;

        static mut VALUE: u64 = 1;

        let val = unsafe { VALUE };
        let mut timeline_submit_info = vk::TimelineSemaphoreSubmitInfo::builder()
            .wait_semaphore_values(std::slice::from_ref(&val))
            .signal_semaphore_values(std::slice::from_ref(&val));

        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(std::slice::from_ref(&frame.cb.raw))
            .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
            .wait_semaphores(std::slice::from_ref(&next_image.acquire_semaphore))
            // .wait_semaphores(&[])
            .signal_semaphores(std::slice::from_ref(
                &next_image.rendering_finished_semaphore,
            ))
            // .signal_semaphores(&[])
            .push_next(&mut timeline_submit_info)
            // a
        ;

        unsafe {
            raw_device.reset_fences(std::slice::from_ref(&fence))?;
            raw_device.queue_submit(
                self.device.queue.raw,
                std::slice::from_ref(&submit_info),
                fence,
            )?;
        }

        unsafe { VALUE += 1 };

        self.swapchain.present(next_image)?;
        self.frame = (self.frame + 1) % FRAMES_IN_FLIGHT;

        Ok(())
    }

    pub fn compute_pipeline(&self, id: usize) -> &ComputePipeline {
        &self.compute_pipelines[id]
    }

    pub fn graphics_pipeline(&self, id: usize) -> &GraphicsPipeline {
        &self.graphics_pipelines[id]
    }

    pub fn register_shader(
        &mut self,
        shader: &'static vulkan::pipeline::ShaderMetadata,
    ) -> ShaderHandle {
        self.shaders.push(shader);
        ShaderHandle(self.shaders.len() - 1)
    }

    pub fn create_graphics_pipeline(
        &mut self,
        vs: ShaderHandle,
        fs: ShaderHandle,
        push_constant_size: usize,
    ) -> anyhow::Result<usize> {
        let pipeline = vulkan::pipeline::create_graphics_pipeline(
            &self.device,
            &[self.shaders[vs.0], self.shaders[fs.0]],
            &self.render_pass,
            &self.descriptor_set_layouts,
            self.swapchain.extent,
            push_constant_size,
        )?;

        let idx = self.graphics_pipelines.len();
        self.graphics_pipelines.push(pipeline);
        Ok(idx)
    }

    pub fn create_compute_pipeline(
        &mut self,
        cs: ShaderHandle,
        push_constant_size: usize,
    ) -> anyhow::Result<usize> {
        let pipeline = vulkan::pipeline::create_compute_pipeline(
            &self.device,
            self.shaders[cs.0],
            &self.descriptor_set_layouts,
            push_constant_size,
        )?;
        let idx = self.compute_pipelines.len();
        self.compute_pipelines.push(pipeline);
        Ok(idx)
    }

    pub fn allocate_descriptor_set(&mut self) -> anyhow::Result<vulkan::DescriptorSet> {
        // destroy old layout - probably should have raii wrapper
        if !self.descriptor_set_layouts.is_empty() {
            for &dsl in self.descriptor_set_layouts.iter() {
                unsafe {
                    self.device.raw.destroy_descriptor_set_layout(dsl, None);
                }
            }
        }

        let (descriptor_set_layouts, descriptor_pool_sizes) =
            vulkan::pipeline::create_descriptor_set_layouts(&self.device, &self.shaders)?;

        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&descriptor_pool_sizes)
            .max_sets(descriptor_set_layouts.len() as u32);

        let descriptor_pool = vulkan::DescriptorPool {
            raw: unsafe {
                self.device
                    .raw
                    .create_descriptor_pool(&descriptor_pool_info, None)
            }?,
            device: Arc::clone(&self.device),
        };

        let descriptor_set_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(descriptor_pool.raw)
            .set_layouts(&descriptor_set_layouts);

        let descriptor_set = vulkan::DescriptorSet {
            raw: unsafe {
                self.device
                    .raw
                    .allocate_descriptor_sets(&descriptor_set_info)
            }?[0],
            // layout: descriptor_set_layouts[0],
            pool: Arc::new(descriptor_pool),
        };

        self.descriptor_set_layouts = descriptor_set_layouts;

        Ok(descriptor_set)
    }

    pub fn create_storage_image(
        &self,
        width: u32,
        height: u32,
    ) -> anyhow::Result<vulkan::descriptors::StorageImage> {
        vulkan::descriptors::create_storage_image(
            &self.device,
            vk::Format::B8G8R8A8_UNORM,
            width,
            height,
        )
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn render_pass(&self) -> &vulkan::pipeline::RenderPass {
        &self.render_pass
    }

    pub fn one_time_submit(
        &self,
        callback: impl FnOnce(vulkan::commands::RecordingCommandBuffer<'_>),
    ) -> anyhow::Result<()> {
        self.device.one_time_submit(callback)
    }

    pub fn create_staging_buffer(&self, size: u64) -> anyhow::Result<Buffer> {
        let usage = vk::BufferUsageFlags::TRANSFER_SRC;
        let memory_properties =
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        self.device.create_buffer(usage, memory_properties, size)
    }

    pub fn create_destination_image(&self, width: u32, height: u32) -> anyhow::Result<Image> {
        self.device.create_image(
            vk::Format::R8_UINT,
            vk::Extent2D::builder().width(width).height(height).build(),
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::STORAGE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
    }

    pub fn create_storage_buffer(&self, size: u64) -> anyhow::Result<Buffer> {
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let memory_properties =
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        self.device.create_buffer(usage, memory_properties, size)
    }
}

impl Drop for RenderBackend {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.device_wait_idle().unwrap();

            for &fence in self.in_flight_fences.iter() {
                self.device.raw.destroy_fence(fence, None);
            }

            if !self.descriptor_set_layouts.is_empty() {
                for &dsl in self.descriptor_set_layouts.iter() {
                    self.device.raw.destroy_descriptor_set_layout(dsl, None);
                }
            }
        }
    }
}

pub use vulkan::{
    descriptors::{ImageDescriptor, StorageImage},
    pipeline::{create_compute_pipeline, create_graphics_pipeline, RenderPass, ShaderMetadata},
    Buffer, ComputePipeline, DescriptorSet, GraphicsPipeline, Image, ImageView,
};

pub(crate) use vulkan::descriptors::update;
pub(crate) use vulkan::pipeline::define_shader;
