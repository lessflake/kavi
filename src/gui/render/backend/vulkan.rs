use std::os::raw::c_char;

macro_rules! cstr {
    ($s:expr) => {
        concat!($s, "\0") as *const str as *const ::std::os::raw::c_char
    };
}

const LAYER_KHRONOS_VALIDATION_LAYER_NAME: *const c_char = cstr!("VK_LAYER_KHRONOS_validation");

mod instance;

pub use instance::Instance;

mod surface;
pub use surface::Surface;

pub(crate) mod physical_device;

mod device;
pub use device::{CommandPool, Device};

mod swapchain;
pub use swapchain::{dxgi_swapchain, DxgiSwapchain, Framebuffer, Swapchain, SwapchainImage};

pub mod pipeline;
pub use pipeline::{ComputePipeline, GraphicsPipeline};

pub mod buffer;
pub use buffer::Buffer;

mod image;
pub use image::{Image, ImageView};

pub mod commands;
pub use commands::CommandBuffer;

pub mod descriptors;
pub use descriptors::{DescriptorPool, DescriptorSet};

pub mod profiling;
