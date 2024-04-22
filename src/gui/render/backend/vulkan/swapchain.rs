use super::{Device, Image, ImageView, Instance, Surface};
use anyhow::Context;
use ash::{extensions::khr, vk};
use raw_window_handle::HasRawWindowHandle;
use std::sync::Arc;

pub struct Swapchain {
    pub raw: vk::SwapchainKHR,
    pub fns: khr::Swapchain,
    pub acquire_semaphores: Vec<vk::Semaphore>,
    pub rendering_finished_semaphores: Vec<vk::Semaphore>,
    next_semaphore: usize,
    pub extent: vk::Extent2D,
    pub format: vk::SurfaceFormatKHR,
    pub present_mode: vk::PresentModeKHR,
    // images: Vec<Image>,
    pub device: Arc<Device>,
}

#[derive(Debug)]
pub enum SwapchainAcquireImageErr {
    RecreateSwapchain,
}

impl Swapchain {
    pub fn new(
        instance: &Instance,
        device: &Arc<Device>,
        surface: &Surface,
    ) -> anyhow::Result<Self> {
        let surface_capabilities = unsafe {
            surface
                .fns
                .get_physical_device_surface_capabilities(device.physical_device.raw, surface.raw)
        }?;

        let extent = surface_capabilities.current_extent;

        let mut image_count = surface_capabilities.min_image_count + 1;
        if surface_capabilities.max_image_count > 0 {
            image_count = image_count.min(surface_capabilities.max_image_count);
        }

        let formats = unsafe {
            surface
                .fns
                .get_physical_device_surface_formats(device.physical_device.raw, surface.raw)
        }?;
        let format = formats
            .into_iter()
            .find(|format| {
                format.format == vk::Format::B8G8R8A8_UNORM
                    && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .context("device does not have suitable surface format")?;

        let present_modes = unsafe {
            surface
                .fns
                .get_physical_device_surface_present_modes(device.physical_device.raw, surface.raw)
        }?;
        let present_mode = present_modes
            .into_iter()
            // "Hardware Legacy Flip" in fullscreen only if this is FIFO or IMMEDIATE
            .find(|&present_mode| present_mode == vk::PresentModeKHR::FIFO)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        let swapchain_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface.raw)
            .min_image_count(image_count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(vk::SwapchainKHR::null())
            .build();

        let fns = khr::Swapchain::new(&instance.raw, &device.raw);
        let swapchain = unsafe { fns.create_swapchain(&swapchain_info, None) }?;

        let acquire_semaphores = (0..image_count as usize)
            .map(|_| {
                let info = vk::SemaphoreCreateInfo::default();
                Ok(unsafe { device.raw.create_semaphore(&info, None) }?)
            })
            .collect::<anyhow::Result<_>>()?;

        let rendering_finished_semaphores = (0..image_count as usize)
            .map(|_| {
                let info = vk::SemaphoreCreateInfo::default();
                Ok(unsafe { device.raw.create_semaphore(&info, None) }?)
            })
            .collect::<anyhow::Result<_>>()?;

        Ok(Self {
            raw: swapchain,
            fns,
            acquire_semaphores,
            rendering_finished_semaphores,
            next_semaphore: 0,
            extent,
            format,
            present_mode,
            device: Arc::clone(device),
        })
    }

    pub fn framebuffers(
        &self,
        device: &Arc<Device>,
        render_pass: &super::pipeline::RenderPass,
    ) -> anyhow::Result<Vec<Framebuffer>> {
        unsafe { self.fns.get_swapchain_images(self.raw) }?
            .into_iter()
            .map(|image| Image {
                raw: image,
                format: self.format.format,
                memory: None,
                extent: self.extent,
                usage: vk::ImageUsageFlags::STORAGE,
                device: Arc::clone(device),
            })
            .map(|image| {
                let view = image.view(self.format.format)?;
                let attachments = [view.raw];
                let framebuffer_info = vk::FramebufferCreateInfo::builder()
                    .render_pass(render_pass.raw)
                    .attachments(&attachments)
                    .width(self.extent.width)
                    .height(self.extent.height)
                    .layers(1);

                let framebuffer =
                    unsafe { device.raw.create_framebuffer(&framebuffer_info, None) }?;

                Ok(Framebuffer {
                    raw: framebuffer,
                    image,
                    view,
                    device: Arc::clone(device),
                })
            })
            .collect::<anyhow::Result<_>>()
    }

    pub fn acquire_next_image(&mut self) -> Result<SwapchainImage, SwapchainAcquireImageErr> {
        let acquire_semaphore = self.acquire_semaphores[self.next_semaphore];
        let rendering_finished_semaphore = self.rendering_finished_semaphores[self.next_semaphore];

        let index = match unsafe {
            self.fns
                .acquire_next_image(self.raw, u64::MAX, acquire_semaphore, vk::Fence::null())
        } {
            Ok((_, suboptimal)) if suboptimal => {
                return Err(SwapchainAcquireImageErr::RecreateSwapchain)
            }
            Ok((idx, _)) => idx,
            Err(e) if e == vk::Result::ERROR_OUT_OF_DATE_KHR => {
                return Err(SwapchainAcquireImageErr::RecreateSwapchain)
            }
            Err(_) => panic!("too lazy to handle this case right now"), // TODO
        };

        self.next_semaphore = (self.next_semaphore + 1) % self.acquire_semaphores.len();

        Ok(SwapchainImage {
            index,
            acquire_semaphore,
            rendering_finished_semaphore,
        })
    }

    pub fn present(&mut self, image: SwapchainImage) -> anyhow::Result<()> {
        let present_info = vk::PresentInfoKHR::builder()
            .swapchains(std::slice::from_ref(&self.raw))
            .image_indices(std::slice::from_ref(&image.index))
            .wait_semaphores(std::slice::from_ref(&image.rendering_finished_semaphore));

        match unsafe { self.fns.queue_present(self.device.queue.raw, &present_info) } {
            Ok(_) => Ok(()),
            Err(e) if e == vk::Result::ERROR_OUT_OF_DATE_KHR => Ok(()),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    }

    pub fn recreate(&mut self, surface: &Surface, extent: vk::Extent2D) -> anyhow::Result<Self> {
        let image_count = self.acquire_semaphores.len();

        let surface_capabilities = unsafe {
            surface.fns.get_physical_device_surface_capabilities(
                self.device.physical_device.raw,
                surface.raw,
            )
        }?;

        // let extent = surface_capabilities.current_extent;
        // println!("recreating swapchain {:?}", extent);

        let swapchain_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface.raw)
            .min_image_count(image_count as _)
            .image_format(self.format.format)
            .image_color_space(self.format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(self.present_mode)
            .clipped(true)
            .old_swapchain(self.raw)
            .build();

        let fns = self.fns.clone();
        let swapchain = unsafe { fns.create_swapchain(&swapchain_info, None) }?;

        Ok(Self {
            raw: swapchain,
            fns,
            acquire_semaphores: std::mem::take(&mut self.acquire_semaphores),
            rendering_finished_semaphores: std::mem::take(&mut self.rendering_finished_semaphores),
            next_semaphore: self.next_semaphore,
            extent,
            format: self.format,
            present_mode: self.present_mode,
            device: Arc::clone(&self.device),
        })
    }
}

pub struct SwapchainImage {
    pub index: u32,
    pub acquire_semaphore: vk::Semaphore,
    pub rendering_finished_semaphore: vk::Semaphore,
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            for &semaphore in self
                .acquire_semaphores
                .iter()
                .chain(self.rendering_finished_semaphores.iter())
            {
                self.device.raw.destroy_semaphore(semaphore, None);
            }
            self.fns.destroy_swapchain(self.raw, None);
        }
    }
}

pub struct Framebuffer {
    pub raw: vk::Framebuffer,
    pub image: Image,
    pub view: ImageView,
    device: Arc<Device>,
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.destroy_framebuffer(self.raw, None);
        }
    }
}

pub fn dxgi_swapchain(
    instance: &Instance,
    device: &Arc<Device>,
    window: &impl HasRawWindowHandle,
    surface: &Surface,
) -> anyhow::Result<DxgiSwapchain> {
    use winapi::Interface as _;

    let lib_dxgi = d3d12::DxgiLib::new()?;
    let lib_d3d12 = d3d12::D3D12Lib::new()?;

    let factory_flags = d3d12::FactoryCreationFlags::empty();
    let (factory, res) = lib_dxgi.create_factory2(factory_flags)?;
    assert_eq!(res, winapi::shared::winerror::S_OK);

    let feature_level = d3d12::FeatureLevel::L11_0;

    let mut device_id_props = vk::PhysicalDeviceIDProperties::builder();
    let mut device_props = vk::PhysicalDeviceProperties2::builder().push_next(&mut device_id_props);
    unsafe {
        instance
            .raw
            .get_physical_device_properties2(device.physical_device.raw, &mut device_props)
    }

    let mut adapter = d3d12::Adapter1::null();
    let uuid = winapi::shared::dxgi::IDXGIAdapter1::uuidof();
    let luid = device_id_props.device_luid.as_ptr() as *const winapi::shared::ntdef::LUID;
    let res = unsafe { factory.EnumAdapterByLuid(*luid, &uuid, adapter.mut_void()) };
    assert_eq!(res, winapi::shared::winerror::S_OK);

    let (d3d12_device, res) = lib_d3d12.create_device(adapter, feature_level)?;
    assert_eq!(res, winapi::shared::winerror::S_OK);

    let (command_queue, res) = d3d12_device.create_command_queue(
        d3d12::CmdListType::Direct,
        d3d12::Priority::Normal,
        d3d12::CommandQueueFlags::empty(),
        0,
    );
    assert_eq!(res, winapi::shared::winerror::S_OK);

    let hwnd = if let raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32Handle {
        hwnd,
        ..
    }) = window.raw_window_handle()
    {
        hwnd
    } else {
        unreachable!()
    };

    let surface_capabilities = unsafe {
        surface
            .fns
            .get_physical_device_surface_capabilities(device.physical_device.raw, surface.raw)
    }?;

    let extent = surface_capabilities.current_extent;

    let mut swapchain = d3d12::SwapChain1::null();

    let image_count = 3;

    let desc = winapi::shared::dxgi1_2::DXGI_SWAP_CHAIN_DESC1 {
        AlphaMode: d3d12::AlphaMode::Ignore as _,
        BufferCount: image_count as u32,
        Width: extent.width,
        Height: extent.height,
        Format: winapi::shared::dxgiformat::DXGI_FORMAT_B8G8R8A8_UNORM,
        Flags: winapi::shared::dxgi::DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT
            | winapi::shared::dxgi::DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING,
        BufferUsage: winapi::shared::dxgitype::DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SampleDesc: winapi::shared::dxgitype::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Scaling: d3d12::Scaling::Identity as _,
        Stereo: false as _,
        SwapEffect: d3d12::SwapEffect::FlipSequential as _,
    };

    let res = unsafe {
        factory.CreateSwapChainForHwnd(
            command_queue.as_mut_ptr() as *mut _,
            hwnd as _,
            &desc,
            core::ptr::null(),
            core::ptr::null_mut(),
            swapchain.mut_void() as *mut *mut _,
        )
    };
    assert_eq!(res, winapi::shared::winerror::S_OK);

    let (swapchain, res): (d3d12::SwapChain3, _) = unsafe { swapchain.cast() };
    assert_eq!(res, winapi::shared::winerror::S_OK);

    // I don't know why these aren't in winapi
    const DXGI_MWA_NO_WINDOW_CHANGES: u32 = 1;
    const DXGI_MWA_NO_ALT_ENTER: u32 = 2;
    let _res = unsafe {
        factory.MakeWindowAssociation(
            hwnd as _,
            DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER,
        )
    };

    // let mut type_info = vk::SemaphoreTypeCreateInfo::builder()
    //     .semaphore_type(vk::SemaphoreType::TIMELINE)
    //     .build();
    // let info = vk::SemaphoreCreateInfo::builder().push_next(&mut type_info);
    // let info = vk::SemaphoreCreateInfo::default();
    let mut type_info = vk::SemaphoreTypeCreateInfo::builder()
        .semaphore_type(vk::SemaphoreType::TIMELINE)
        .build();
    let info = vk::SemaphoreCreateInfo::builder().push_next(&mut type_info);
    let acquire_semaphore = unsafe { device.raw.create_semaphore(&info, None) }?;
    // let info = vk::SemaphoreCreateInfo::default();
    let mut type_info = vk::SemaphoreTypeCreateInfo::builder()
        .semaphore_type(vk::SemaphoreType::TIMELINE)
        .build();
    let info = vk::SemaphoreCreateInfo::builder().push_next(&mut type_info);
    let rendering_finished_semaphore = unsafe { device.raw.create_semaphore(&info, None) }?;

    Ok(DxgiSwapchain {
        raw: swapchain,
        image_count,
        extent,
        acquire_semaphore,
        rendering_finished_semaphore,
        d3d12_device,
        lib_dxgi,
        lib_d3d12,
        // dxgi_debug,
        // d3d12_debug,
        // d3d12_info_queue,
        shared_handles: Vec::new(),
        resources: Vec::new(),
        device: Arc::clone(device),
    })
}

pub struct DxgiSwapchain {
    raw: d3d12::SwapChain3,
    pub image_count: usize,
    pub extent: vk::Extent2D,
    acquire_semaphore: vk::Semaphore,
    rendering_finished_semaphore: vk::Semaphore,
    d3d12_device: d3d12::Device,
    lib_dxgi: d3d12::DxgiLib,
    lib_d3d12: d3d12::D3D12Lib,
    // dxgi_debug: d3d12::InfoQueue,
    // d3d12_debug: d3d12::Debug,
    // d3d12_info_queue: d3d12::WeakPtr<winapi::um::d3d12sdklayers::ID3D12InfoQueue>,
    shared_handles: Vec<vk::HANDLE>,
    resources: Vec<d3d12::Resource>,
    device: Arc<Device>,
}

impl Drop for DxgiSwapchain {
    fn drop(&mut self) {
        unsafe {
            self.device
                .raw
                .destroy_semaphore(self.acquire_semaphore, None);
            self.device
                .raw
                .destroy_semaphore(self.rendering_finished_semaphore, None);
        }
    }
}

impl DxgiSwapchain {
    // Waits on `rendering_finished_semaphore` before presenting
    pub fn present(&self, _next_image: SwapchainImage) -> anyhow::Result<()> {
        let wait_info = vk::SemaphoreWaitInfo::builder()
            .semaphores(std::slice::from_ref(
                &_next_image.rendering_finished_semaphore,
            ))
            .values(&[0]);
        unsafe { self.device.raw.wait_semaphores(&wait_info, 0) }?;
        let err = unsafe {
            self.raw
                .Present(0, winapi::shared::dxgi::DXGI_PRESENT_ALLOW_TEARING)
            // .Present(0, 0)
        };
        // unsafe {
        //     self.device
        //         .raw
        //         .destroy_semaphore(self.acquire_semaphore, None)
        // };
        assert_eq!(err, winapi::shared::winerror::S_OK);
        Ok(())
    }

    // Signals `acquire_semaphore`
    pub fn acquire_next_image(&mut self) -> Result<SwapchainImage, SwapchainAcquireImageErr> {
        // let info = vk::SemaphoreCreateInfo::default();
        // let mut type_info = vk::SemaphoreTypeCreateInfo::builder()
        //     .semaphore_type(vk::SemaphoreType::TIMELINE)
        //     .build();
        // let info = vk::SemaphoreCreateInfo::builder().push_next(&mut type_info);
        // self.acquire_semaphore = unsafe { self.device.raw.create_semaphore(&info, None) }.unwrap();
        static mut VALUE: u64 = 1;
        let val = unsafe { VALUE };

        let index = self.raw.get_current_back_buffer_index();
        let info = vk::SemaphoreSignalInfo::builder()
            .semaphore(self.acquire_semaphore)
            .value(val);
        unsafe { VALUE += 1 };
        unsafe { self.device.raw.signal_semaphore(&info) }.unwrap();
        Ok(SwapchainImage {
            index,
            acquire_semaphore: self.acquire_semaphore,
            rendering_finished_semaphore: self.rendering_finished_semaphore,
        })
    }

    pub fn recreate(&mut self, extent: vk::Extent2D) {
        unsafe { self.device.raw.device_wait_idle().unwrap() };
        for handle in self.shared_handles.drain(..) {
            let err = unsafe { winapi::um::handleapi::CloseHandle(handle as _) };
            assert!(err != 0);
        }
        for resource in self.resources.drain(..) {
            unsafe { resource.destroy() };
        }
        self.extent = extent;
        let err = unsafe {
            self.raw.ResizeBuffers(
                0,
                extent.width,
                extent.height,
                winapi::shared::dxgiformat::DXGI_FORMAT_UNKNOWN,
                winapi::shared::dxgi::DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT
                    | winapi::shared::dxgi::DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING,
            )
        };
        assert_eq!(err, winapi::shared::winerror::S_OK);
    }

    pub fn create_images(
        &mut self,
        device: &Arc<Device>,
        render_pass: &super::pipeline::RenderPass,
    ) -> anyhow::Result<Vec<Framebuffer>> {
        use winapi::Interface;
        (0..self.image_count)
            .map(|i| {
                let mut res = d3d12::Resource::null();
                unsafe {
                    self.raw.GetBuffer(
                        i as _,
                        &winapi::um::d3d12::ID3D12Resource::uuidof(),
                        res.mut_void(),
                    );
                }

                let mut handle = unsafe { std::mem::zeroed() };
                let (object, err) = unsafe { res.cast::<winapi::um::d3d12::ID3D12DeviceChild>() };
                assert_eq!(err, winapi::shared::winerror::S_OK);
                let err = unsafe {
                    self.d3d12_device.CreateSharedHandle(
                        object.as_mut_ptr(),
                        core::ptr::null(),
                        winapi::um::winnt::GENERIC_ALL,
                        core::ptr::null(),
                        &mut handle,
                    )
                };
                assert_eq!(err, winapi::shared::winerror::S_OK);
                self.shared_handles.push(handle as _);

                let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT;
                let format = vk::Format::B8G8R8A8_UNORM;

                let mut external_memory_image_info = vk::ExternalMemoryImageCreateInfo::builder()
                    .handle_types(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE);
                let image_info = vk::ImageCreateInfo::builder()
                    .push_next(&mut external_memory_image_info)
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(
                        vk::Extent3D::builder()
                            .width(self.extent.width)
                            .height(self.extent.height)
                            .depth(1)
                            .build(),
                    )
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .queue_family_indices(&[0])
                    .initial_layout(vk::ImageLayout::UNDEFINED);

                let image = unsafe { device.raw.create_image(&image_info, None) }?;
                let memory_requirements =
                    unsafe { device.raw.get_image_memory_requirements(image) };
                let fns = khr_ext::ExternalMemoryWin32::new(&device.instance.raw, &device.raw);
                let memory_properties = unsafe {
                    fns.get_memory_win32_handle_properties(
                        vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE,
                        handle as _,
                    )
                }?;

                let memory_type = device
                    .find_memory_type_index(
                        memory_properties.memory_type_bits,
                        vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    )
                    .context("no appropriate memory type")?;

                let memory_dedicated_allocate_info = vk::MemoryDedicatedAllocateInfo::builder()
                    .image(image)
                    .build();

                let mut win32_memory_info = vk::ImportMemoryWin32HandleInfoKHR::builder()
                    .handle(handle as _)
                    .handle_type(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE);
                win32_memory_info.p_next = &memory_dedicated_allocate_info as *const _ as *const _;

                let memory_allocate_info = vk::MemoryAllocateInfo::builder()
                    .allocation_size(memory_requirements.size)
                    .memory_type_index(memory_type)
                    .push_next(&mut win32_memory_info);

                let memory = unsafe { device.raw.allocate_memory(&memory_allocate_info, None) }?;

                unsafe { device.raw.bind_image_memory(image, memory, 0) }?;

                let image = Image {
                    raw: image,
                    memory: Some(memory),
                    format,
                    extent: self.extent,
                    usage,
                    device: Arc::clone(device),
                };

                let view = image.view(format)?;
                let framebuffer_info = vk::FramebufferCreateInfo::builder()
                    .render_pass(render_pass.raw)
                    .attachments(std::slice::from_ref(&view.raw))
                    .width(self.extent.width)
                    .height(self.extent.height)
                    .layers(1);

                let framebuffer =
                    unsafe { device.raw.create_framebuffer(&framebuffer_info, None) }?;

                self.resources.push(res);
                unsafe { res.destroy() };
                Ok(Framebuffer {
                    raw: framebuffer,
                    image,
                    view,
                    device: Arc::clone(device),
                })
            })
            .collect()
    }
}

mod khr_ext {
    use ash::{
        prelude::*,
        vk::{self, HANDLE},
        Device, Instance,
    };
    use std::{ffi::CStr, mem};

    #[derive(Clone)]
    pub struct ExternalMemoryWin32 {
        handle: vk::Device,
        fns: vk::KhrExternalMemoryWin32Fn,
    }

    impl ExternalMemoryWin32 {
        pub fn new(instance: &Instance, device: &Device) -> Self {
            let fns = vk::KhrExternalMemoryWin32Fn::load(|name| unsafe {
                mem::transmute(instance.get_device_proc_addr(device.handle(), name.as_ptr()))
            });
            Self {
                handle: device.handle(),
                fns,
            }
        }

        pub fn name() -> &'static CStr {
            vk::KhrExternalMemoryWin32Fn::name()
        }

        // #[doc = "<https://www.khronos.org/registry/vulkan/specs/1.3-extensions/man/html/vkGetMemoryWin32HandleKHR.html>"]
        pub unsafe fn get_memory_win32_handle(
            &self,
            create_info: &vk::MemoryGetWin32HandleInfoKHR,
        ) -> HANDLE {
            todo!();
        }

        // #[doc = "<https://www.khronos.org/registry/vulkan/specs/1.3-extensions/man/html/vkGetMemoryWin32HandlePropertiesKHR.html>"]
        pub unsafe fn get_memory_win32_handle_properties(
            &self,
            handle_type: vk::ExternalMemoryHandleTypeFlags,
            handle: HANDLE,
        ) -> VkResult<vk::MemoryWin32HandlePropertiesKHR> {
            let mut memory_win32_handle_properties = vk::MemoryWin32HandlePropertiesKHR::default();
            let err_code = self.fns.get_memory_win32_handle_properties_khr(
                self.handle,
                handle_type,
                handle,
                &mut memory_win32_handle_properties,
            );
            match err_code {
                vk::Result::SUCCESS => Ok(memory_win32_handle_properties),
                _ => Err(err_code),
            }
        }

        pub fn fp(&self) -> &vk::KhrExternalMemoryWin32Fn {
            &self.fns
        }

        pub fn device(&self) -> vk::Device {
            self.handle
        }
    }
}

#[cfg(test)]
mod test {
    use crate::gui::render::backend::vulkan::{self, physical_device::PhysicalDeviceIterExt as _};
    use anyhow::Context;
    use ash::vk;

    #[test]
    fn dxgi_swapchain_test() -> anyhow::Result<()> {
        let debug = true;

        let window = crate::gui::window::Window::start_with_thread(480, 360)?;

        let instance = vulkan::Instance::builder()
            .extensions(ash_window::enumerate_required_extensions(&window)?)
            .debug(debug)
            .build()?;

        let surface = vulkan::Surface::create(&instance, &window)?;

        let pdevice = vulkan::physical_device::enumerate_physical_devices(&instance)?
            .with_presentation_support(&surface)
            .next()
            .context("no suitable device found")?;

        let device = vulkan::Device::create(instance.clone(), pdevice, debug)?;

        let render_pass =
            vulkan::pipeline::create_render_pass(&device, vk::Format::B8G8R8A8_UNORM)?;

        let testy_swapchain = vulkan::dxgi_swapchain(&instance, &device, &window, &surface)?;
        let dxgi_framebuffers = testy_swapchain.create_images(&device, &render_pass)?;
        println!("{} dxgi framebuffers", dxgi_framebuffers.len());

        Ok(())
    }
}
