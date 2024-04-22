use std::sync::Arc;

use super::Device;
use ash::vk;

pub trait Pipeline {
    const BIND_POINT: vk::PipelineBindPoint;
    const SHADER_STAGE: vk::ShaderStageFlags;
    fn common(&self) -> &super::pipeline::PipelineCommon;
}

impl Pipeline for ComputePipeline {
    const BIND_POINT: vk::PipelineBindPoint = vk::PipelineBindPoint::COMPUTE;
    const SHADER_STAGE: vk::ShaderStageFlags = vk::ShaderStageFlags::COMPUTE;

    fn common(&self) -> &super::pipeline::PipelineCommon {
        &self.common
    }
}

impl Pipeline for GraphicsPipeline {
    const BIND_POINT: vk::PipelineBindPoint = vk::PipelineBindPoint::GRAPHICS;
    const SHADER_STAGE: vk::ShaderStageFlags = vk::ShaderStageFlags::ALL_GRAPHICS;

    fn common(&self) -> &super::pipeline::PipelineCommon {
        &self.common
    }
}

pub struct PipelineCommon {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub shaders: Vec<(&'static ShaderMetadata, vk::ShaderModule)>, // TODO: tuple not beautiful
    pub push_constant_bytes: usize,

    pub device: Arc<Device>,
}

impl Drop for PipelineCommon {
    fn drop(&mut self) {
        unsafe {
            for &(_, shader) in self.shaders.iter() {
                self.device.raw.destroy_shader_module(shader, None);
            }
            self.device
                .raw
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.raw.destroy_pipeline(self.pipeline, None);
        }
    }
}

pub struct ComputePipeline {
    pub common: PipelineCommon,
}

const SHADER_DATA: &[u8] = include_bytes!(
    "../../../../shaders/target/spirv-unknown-vulkan1.2/release/deps/shaders.spv.dir/module"
);

pub struct ShaderMetadata {
    pub entry: &'static str,
    pub stage: vk::ShaderStageFlags,
    pub descriptors: &'static [(u32, u32, vk::DescriptorType, u32)],
    pub data: &'static [u8],
}

impl ShaderMetadata {
    fn entry_point(&'static self) -> &std::ffi::CStr {
        std::ffi::CStr::from_bytes_with_nul(self.entry.as_bytes()).unwrap()
    }
}

macro_rules! _define_shader {
    ($name:ident, $entry:literal, $stage:expr, $data:expr $(,$descriptor:expr)* $(,)?) => {
        pub const $name: $crate::gui::render::backend::ShaderMetadata = $crate::gui::render::backend::ShaderMetadata {
            entry: concat!($entry, "\0"),
            stage: $stage,
            descriptors: &[$($descriptor),*],
            data: $data,
        };
    };
}
pub(crate) use _define_shader as define_shader;

pub fn create_descriptor_set_layouts(
    device: &Device,
    shaders: &[&'static ShaderMetadata],
) -> anyhow::Result<(Vec<vk::DescriptorSetLayout>, Vec<vk::DescriptorPoolSize>)> {
    // Each shader knows which descriptors it uses, plus metadata about the descriptors:
    // which set they belong to, which binding within the set, and the count.
    // To fill out the descriptor set layout, we need:
    // How many descriptor sets there are
    // How many bindings are in each descriptor
    // How many descriptors in each binding
    // The type of the descriptors in each binding

    use std::collections::btree_map::{BTreeMap, Entry};
    let mut map = BTreeMap::new();
    let mut set_count = 0;
    for (stage, &(set, binding, kind, count)) in shaders
        .iter()
        .flat_map(|s| s.descriptors.iter().map(|d| (s.stage, d)))
    {
        if set > set_count {
            set_count = set
        };
        match map.entry((set, binding)) {
            Entry::Vacant(e) => {
                e.insert((stage, kind, count));
            }
            Entry::Occupied(mut e) => {
                if matches!(e.get(), &(_, k, c) if k == kind && c == count) {
                    e.get_mut().0 |= stage;
                } else {
                    anyhow::bail!("there are conflicting descriptors")
                }
            }
        }
    }
    set_count += 1;

    let mut descriptor_pool_sizes: Vec<vk::DescriptorPoolSize> = Vec::new();

    // TODO what happens if shaders only reference i.e. sets 0 and 2, there is no set 1?
    // Do we just make an empty set layout and that's that?
    Ok((0..set_count)
        .map(|set| {
            let bindings: Vec<_> = map
                .iter()
                .filter(|(&(s, _), _)| s == set)
                .map(|(&(_, binding), &(stages, kind, count))| {
                    if let Some(mut dps) = descriptor_pool_sizes
                        .iter_mut()
                        .find(|item| item.ty == kind)
                    {
                        dps.descriptor_count += count;
                    } else {
                        descriptor_pool_sizes.push(vk::DescriptorPoolSize {
                            ty: kind,
                            descriptor_count: count,
                        })
                    }

                    vk::DescriptorSetLayoutBinding::builder()
                        .binding(binding)
                        .descriptor_count(count)
                        .descriptor_type(kind)
                        .stage_flags(stages)
                        .build()
                })
                .collect();
            let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
            unsafe { device.raw.create_descriptor_set_layout(&info, None) }
        })
        .collect::<Result<_, _>>()
        .map(|v| (v, descriptor_pool_sizes))?)
}

pub fn create_compute_pipeline(
    device: &Arc<Device>,
    shader: &'static ShaderMetadata,
    descriptor_set_layouts: &[vk::DescriptorSetLayout],
    push_constant_bytes: usize,
) -> anyhow::Result<ComputePipeline> {
    let code = ash::util::read_spv(&mut std::io::Cursor::new(SHADER_DATA))?;
    let module_info = vk::ShaderModuleCreateInfo::builder().code(&code).build();
    let module = unsafe { device.raw.create_shader_module(&module_info, None) }?;

    let stage_info = vk::PipelineShaderStageCreateInfo::builder()
        .stage(shader.stage)
        .module(module)
        .name(shader.entry_point())
        .build();

    let mut layout_info =
        vk::PipelineLayoutCreateInfo::builder().set_layouts(descriptor_set_layouts);

    let push_constant_ranges = vk::PushConstantRange::builder()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .offset(0)
        .size(push_constant_bytes as _);

    if push_constant_bytes > 0 {
        layout_info = layout_info.push_constant_ranges(std::slice::from_ref(&push_constant_ranges));
    }

    let layout = unsafe { device.raw.create_pipeline_layout(&layout_info, None) }?;

    let pipeline_info = vk::ComputePipelineCreateInfo::builder()
        .stage(stage_info)
        .layout(layout)
        .build();

    let pipeline = unsafe {
        device
            .raw
            .create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .map_err(|_| anyhow::anyhow!("failed to create compute pipeline"))
    }?[0];

    let common = PipelineCommon {
        pipeline,
        pipeline_layout: layout,
        push_constant_bytes,
        // descriptor_set_layouts,
        shaders: vec![(shader, module)],
        device: Arc::clone(device),
    };
    Ok(ComputePipeline { common })
}

pub struct GraphicsPipeline {
    pub common: PipelineCommon,
}

pub fn create_graphics_pipeline(
    device: &Arc<Device>,
    shaders: &[&'static ShaderMetadata],
    render_pass: &RenderPass,
    descriptor_set_layouts: &[vk::DescriptorSetLayout],
    extent: vk::Extent2D,
    push_constant_bytes: usize,
) -> anyhow::Result<GraphicsPipeline> {
    let shader_modules: Vec<_> = shaders
        .iter()
        .map(|&shader| {
            let code = ash::util::read_spv(&mut std::io::Cursor::new(shader.data))?;
            let module_info = vk::ShaderModuleCreateInfo::builder().code(&code).build();
            Ok((shader, unsafe {
                device.raw.create_shader_module(&module_info, None)
            }?))
        })
        .collect::<anyhow::Result<_>>()?;

    let shader_stage_infos: Vec<_> = shader_modules
        .iter()
        .map(|&(shader, module)| {
            Ok(vk::PipelineShaderStageCreateInfo::builder()
                .stage(shader.stage)
                .module(module)
                .name(shader.entry_point())
                .build())
        })
        .collect::<anyhow::Result<_>>()?;

    // let descriptor_set_layouts =
    //     create_descriptor_set_layouts(device, &[VERTEX_SHADER, FRAGMENT_SHADER])?;

    let mut layout_info =
        vk::PipelineLayoutCreateInfo::builder().set_layouts(descriptor_set_layouts);

    let push_constant_ranges = vk::PushConstantRange::builder()
        .stage_flags(vk::ShaderStageFlags::ALL_GRAPHICS)
        .offset(0)
        .size(push_constant_bytes as _);

    if push_constant_bytes > 0 {
        layout_info = layout_info.push_constant_ranges(std::slice::from_ref(&push_constant_ranges));
    }

    let layout = unsafe { device.raw.create_pipeline_layout(&layout_info, None) }?;

    let vertex_input_state_info = vk::PipelineVertexInputStateCreateInfo::builder();
    let input_assembly_state_info = vk::PipelineInputAssemblyStateCreateInfo::builder()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewports = [vk::Viewport::builder()
        .x(0.0)
        .y(0.0)
        .width(extent.width as f32)
        .height(extent.height as f32)
        .min_depth(0.0)
        .max_depth(1.0)
        .build()];
    let scissors = [vk::Rect2D::builder()
        .offset(vk::Offset2D::builder().x(0).y(0).build())
        .extent(
            vk::Extent2D::builder()
                .width(extent.width)
                .height(extent.height)
                .build(),
        )
        .build()];
    let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
        .viewports(&viewports)
        .scissors(&scissors);

    let rasterization_state_info = vk::PipelineRasterizationStateCreateInfo::builder()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .line_width(1.0)
        .cull_mode(vk::CullModeFlags::FRONT)
        .front_face(vk::FrontFace::CLOCKWISE)
        .depth_bias_enable(false);

    let multisample_state_info = vk::PipelineMultisampleStateCreateInfo::builder()
        .sample_shading_enable(false)
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);

    let depth_stencil_state_info = vk::PipelineDepthStencilStateCreateInfo::builder()
        .depth_test_enable(false)
        .depth_write_enable(false)
        .depth_bounds_test_enable(false)
        .stencil_test_enable(false);

    let color_blend_attachment_states = [vk::PipelineColorBlendAttachmentState::builder()
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        )
        .blend_enable(false)
        .build()];

    let color_blend_state_info = vk::PipelineColorBlendStateCreateInfo::builder()
        .logic_op_enable(false)
        .attachments(&color_blend_attachment_states);

    let dynamic_state_info = vk::PipelineDynamicStateCreateInfo::builder();

    let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
        .stages(&shader_stage_infos)
        .vertex_input_state(&vertex_input_state_info)
        .input_assembly_state(&input_assembly_state_info)
        .viewport_state(&viewport_state_info)
        .rasterization_state(&rasterization_state_info)
        .multisample_state(&multisample_state_info)
        .depth_stencil_state(&depth_stencil_state_info)
        .color_blend_state(&color_blend_state_info)
        .dynamic_state(&dynamic_state_info)
        .layout(layout)
        .render_pass(render_pass.raw)
        .build();

    let pipeline = unsafe {
        device
            .raw
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .map_err(|_| anyhow::anyhow!("failed to create compute pipeline"))
    }?[0];

    Ok(GraphicsPipeline {
        common: PipelineCommon {
            pipeline,
            pipeline_layout: layout,
            push_constant_bytes,
            // descriptor_set_layouts,
            shaders: shader_modules,
            device: Arc::clone(device),
        },
    })
}

pub struct RenderPass {
    pub raw: vk::RenderPass,
    device: Arc<Device>,
}

impl Drop for RenderPass {
    fn drop(&mut self) {
        unsafe {
            self.device.raw.destroy_render_pass(self.raw, None);
        }
    }
}

pub fn create_render_pass(device: &Arc<Device>, format: vk::Format) -> anyhow::Result<RenderPass> {
    let attachment_descriptions = [vk::AttachmentDescription::builder()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .build()];

    let color_attachment_references = [vk::AttachmentReference::builder()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .build()];

    let subpass_descriptions = [vk::SubpassDescription::builder()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment_references)
        .build()];

    let subpass_dependencies = [vk::SubpassDependency::builder()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .build()];

    let render_pass_info = vk::RenderPassCreateInfo::builder()
        .attachments(&attachment_descriptions)
        .subpasses(&subpass_descriptions)
        .dependencies(&subpass_dependencies)
        .build();

    let render_pass = unsafe { device.raw.create_render_pass(&render_pass_info, None) }?;

    Ok(RenderPass {
        raw: render_pass,
        device: Arc::clone(device),
    })
}
