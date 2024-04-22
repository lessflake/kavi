mod backend;

use backend::RenderBackend;
use raw_window_handle::HasRawWindowHandle;

mod glyph_atlas;

const SHADER_DATA: &[u8] =
    include_bytes!("../shaders/target/spirv-unknown-vulkan1.2/release/deps/shaders.spv.dir/module");

// backend::define_shader! {
//     COMPUTE_SHADER, "main_cs", ash::vk::ShaderStageFlags::COMPUTE, SHADER_DATA,
//     (0, 0, ash::vk::DescriptorType::STORAGE_IMAGE, 1),
// }

backend::define_shader! {
    COMPUTE_SHADER, "cs_with_font", ash::vk::ShaderStageFlags::COMPUTE, SHADER_DATA,
    (0, 0, ash::vk::DescriptorType::STORAGE_IMAGE, 1),
    (0, 1, ash::vk::DescriptorType::STORAGE_IMAGE, 1),
    (0, 2, ash::vk::DescriptorType::STORAGE_BUFFER, 1),
}

// backend::define_shader! {
//     FB_COMPUTE_SHADER, "fb_cs", ash::vk::ShaderStageFlags::COMPUTE, SHADER_DATA,
//     (0, 0, ash::vk::DescriptorType::STORAGE_IMAGE, 3),
// }

backend::define_shader! {
    VERTEX_SHADER, "main_vs", ash::vk::ShaderStageFlags::VERTEX, SHADER_DATA,
}

backend::define_shader! {
    FRAGMENT_SHADER, "main_fs", ash::vk::ShaderStageFlags::FRAGMENT, SHADER_DATA,
    (0, 0, ash::vk::DescriptorType::STORAGE_IMAGE, 1),
}

#[repr(C)]
struct CharEntry {
    atlas_x: u32,
    atlas_y: u32,
    pos_x: u32,
    pos_y: u32,
}

#[allow(dead_code)]
pub struct Render {
    backend: RenderBackend,

    compute_pipeline: usize,
    graphics_pipeline: usize,
    storage_image: backend::StorageImage,

    // Remake descriptor set every time we add or remove a window..?
    // No. Either we don't use a storage image for every window,
    // or we go bindless.
    // Do we *actually* need a storage image per window?
    descriptor_set: backend::DescriptorSet,

    atlas: glyph_atlas::GlyphAtlas,
    text_buffer: backend::Buffer,
}

impl Render {
    pub fn new(window: &impl HasRawWindowHandle) -> anyhow::Result<Self> {
        let mut backend = RenderBackend::new(window)?;

        let cs = backend.register_shader(&COMPUTE_SHADER);
        let vs = backend.register_shader(&VERTEX_SHADER);
        let fs = backend.register_shader(&FRAGMENT_SHADER);

        let descriptor_set = backend.allocate_descriptor_set()?;

        let compute_pipeline = backend.create_compute_pipeline(cs, 0)?;
        let graphics_pipeline = backend.create_graphics_pipeline(vs, fs, 0)?;
        let storage_image = backend.create_storage_image(1280, 720)?;

        backend::update!(descriptor_set, 0;0 => storage_image);

        let atlas = glyph_atlas::GlyphAtlas::new(&backend)?;
        backend::update!(descriptor_set, 1;0 => atlas);

        // let text = TEXT;
        let buffer =
            backend.create_storage_buffer((std::mem::size_of::<CharEntry>() * 10000) as u64)?;

        // for frame in backend.frames() {
        //     frame.cb.record(|cb| {
        //         cb.bind_pipeline(backend.compute_pipeline(compute_pipeline));
        //         cb.bind_pipeline(backend.graphics_pipeline(graphics_pipeline));
        //         cb.bind_descriptor_set(backend.compute_pipeline(compute_pipeline), &descriptor_set);
        //         cb.bind_descriptor_set(
        //             backend.graphics_pipeline(graphics_pipeline),
        //             &descriptor_set,
        //         );
        //         cb.dispatch(TEXT.chars().count() as u32, 1, 1);
        //         cb.with_render_pass(&backend.render_pass(), &frame.fb, |cb| cb.draw(6, 0));
        //     })?;
        // }

        Ok(Self {
            backend,
            compute_pipeline,
            graphics_pipeline,
            storage_image,
            descriptor_set,
            atlas,
            text_buffer: buffer,
        })
    }

    pub fn draw_frame(&mut self, text: &ropey::Rope) -> anyhow::Result<()> {
        let (_, next_image, _) = self.backend.begin_frame()?;

        self.update_buffer(&text)?;
        let frame = &self.backend.frames()[next_image.index as usize];

        let graphics_pipeline = self.backend.graphics_pipeline(self.graphics_pipeline);
        let compute_pipeline = self.backend.compute_pipeline(self.compute_pipeline);
        let render_pass = self.backend.render_pass();

        frame.cb.record(|cb| {
            cb.bind_pipeline(compute_pipeline);
            cb.bind_pipeline(graphics_pipeline);
            cb.bind_descriptor_set(compute_pipeline, &self.descriptor_set);
            cb.bind_descriptor_set(graphics_pipeline, &self.descriptor_set);
            cb.dispatch(text.len_chars() as u32, 1, 1);
            cb.with_render_pass(render_pass, &frame.fb, |cb| cb.draw(6, 0));
        })?;

        self.backend.draw_frame(next_image)?;

        // let gpu_duration = self.backend.profiler.retrieve_previous_result()?;
        // log::info!("{}us", gpu_duration.as_micros());
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        self.backend.recreate_swapchain(width, height)
    }

    fn update_buffer(&mut self, text: &ropey::Rope) -> anyhow::Result<()> {
        let chars = self
            .text_buffer
            .map_memory::<CharEntry>(0, text.chars().count())?;

        let mut x = 0;
        let mut y = 0;
        let mut n = 0;
        for c in text.chars() {
            let (atlas_x, atlas_y) = match self.atlas.get(c) {
                Some(e) => e,
                None => {
                    if c == '\n' {
                        y += 1;
                        x = 0;
                    }
                    continue;
                }
            };

            chars[n] = CharEntry {
                atlas_x: atlas_x as u32,
                atlas_y: atlas_y as u32,
                pos_x: x,
                pos_y: y,
            };

            x += 1;
            n += 1;
        }

        self.text_buffer.unmap_memory();

        self.descriptor_set.write_buffer(2, 0, &self.text_buffer);
        Ok(())
    }
}
