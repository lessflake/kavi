use super::backend::{Image, ImageView};
use std::collections::{hash_map::Entry, HashMap};

pub struct GlyphAtlas {
    image: Image,
    view: ImageView,
    next_idx: u16,
    map: HashMap<char, u16>,
    dims: [u16; 2],
    glyph_dims: [u16; 2],
    // some lru cache mechanism
}

impl GlyphAtlas {
    pub fn new(ctx: &super::backend::RenderBackend) -> anyhow::Result<Self> {
        let font = bdf::open("../../../fonts/creep2-11.bdf")?;

        let ascent = if let Some(bdf::Property::Integer(x)) = font.properties().get("FONT_ASCENT") {
            *x
        } else {
            anyhow::bail!("font lacks required metadata");
        };

        let glyph_width = font.bounds().width;
        let glyph_height = font.bounds().height;
        let glyph_count = font.glyphs().len() as u32;

        assert!(glyph_count > 0);

        let texture_width = std::iter::successors(Some(1), |&n| Some(2 * n))
            .find(|x| (x / glyph_width) * (x / glyph_height) >= glyph_count)
            .unwrap();

        let glyphs_per_row = texture_width / glyph_width;
        let rows_used = (glyph_count + glyphs_per_row - 1) / glyphs_per_row;

        let texture_height = std::iter::successors(Some(texture_width), |n| Some(n / 2))
            .find(|n| ((n / 2) + glyph_height - 1) / glyph_height < rows_used)
            .unwrap();

        let glyphs_per_line = texture_width / glyph_width;

        let mut staging_buffer =
            ctx.create_staging_buffer(texture_width as u64 * texture_height as u64)?;
        let data =
            staging_buffer.map_memory::<u8>(0, texture_width as usize * texture_height as usize)?;
        let mut map = HashMap::new();

        let origin_x = 0i32;
        let origin_y = ascent as i32;

        for (i, glyph) in font.glyphs().values().enumerate() {
            let i = i as u32;
            let tile_x = i % glyphs_per_line;
            let tile_y = i / glyphs_per_line;

            let anchor_x = origin_x + glyph.bounds().x;
            let anchor_y = origin_y - glyph.bounds().y - glyph.bounds().height as i32;

            for ((glyph_x, glyph_y), _) in glyph.pixels().filter(|&(_, v)| v) {
                let x = anchor_x + glyph_x as i32;
                let y = anchor_y + glyph_y as i32;

                let pixel_x = glyph_width * tile_x + x as u32;
                let pixel_y = glyph_height * tile_y + y as u32;
                let pixel_index = pixel_x as usize + (texture_width * pixel_y) as usize;

                data[pixel_index] = u8::MAX;
            }

            map.insert(glyph.codepoint(), i as u16);
        }

        staging_buffer.unmap_memory();

        let image = ctx.create_destination_image(texture_width, texture_height)?;

        ctx.one_time_submit(|cb| {
            cb.image_barrier(
                &image,
                ash::vk::AccessFlags::empty(),
                ash::vk::AccessFlags::TRANSFER_WRITE,
                ash::vk::ImageLayout::UNDEFINED,
                ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ash::vk::PipelineStageFlags::TOP_OF_PIPE,
                ash::vk::PipelineStageFlags::TRANSFER,
            );

            cb.copy_buffer_to_image(&staging_buffer, &image);

            cb.image_barrier(
                &image,
                ash::vk::AccessFlags::TRANSFER_WRITE,
                ash::vk::AccessFlags::SHADER_READ,
                ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ash::vk::ImageLayout::GENERAL,
                ash::vk::PipelineStageFlags::TRANSFER,
                ash::vk::PipelineStageFlags::COMPUTE_SHADER,
            );
        })?;

        let view = image.view(ash::vk::Format::R8_UINT)?;

        Ok(Self {
            image,
            view,
            next_idx: 0,
            map,
            dims: [texture_width as u16, texture_height as u16],
            glyph_dims: [glyph_width as u16, glyph_height as u16],
        })
    }

    pub fn idx_to_coords(&self, idx: u16) -> (u16, u16) {
        let row_length = self.dims[0] / self.glyph_dims[0];
        let x = idx % row_length;
        let y = idx / row_length;
        (x, y)
    }

    fn write(&self, c: char, idx: u16) {
        // going to need to have the font loaded up for this

        // map staging buffer memory, write to it
        // queue up the transfer
    }

    // fn insert(&mut self, c: char) -> u16 {
    //     let idx = self.next_idx;
    //     self.next_idx += 1;
    //     idx
    // }

    pub fn get(&self, c: char) -> Option<(u16, u16)> {
        self.map.get(&c).map(|&idx| self.idx_to_coords(idx))
    }

    // pub fn map(&mut self, c: char) -> u16 {
    //     match self.map.entry(c) {
    //         Entry::Occupied(e) => *e.get(),
    //         Entry::Vacant(e) => {
    //             let idx = self.next_idx;
    //             e.insert(idx);
    //             self.next_idx += 1;
    //             idx
    //         }
    //     }
    // }
}

// TODO: a bit of a hack?
impl super::backend::ImageDescriptor for GlyphAtlas {
    fn kind() -> ash::vk::DescriptorType {
        ash::vk::DescriptorType::STORAGE_IMAGE
    }

    fn info(&self) -> ash::vk::DescriptorImageInfo {
        ash::vk::DescriptorImageInfo::builder()
            .image_layout(ash::vk::ImageLayout::GENERAL)
            .image_view(self.view.raw)
            .build()
    }
}

#[test]
fn parse_bdf() {
    let font = bdf::open("fonts/creep2-11.bdf").unwrap();

    let ascent = if let bdf::Property::Integer(x) = font.properties().get("FONT_ASCENT").unwrap() {
        *x
    } else {
        panic!()
    };

    let glyph_width = font.bounds().width;
    let glyph_height = font.bounds().height;
    let glyph_count = font.glyphs().len() as u32;

    assert!(glyph_count > 0);

    let texture_width = std::iter::successors(Some(1), |&n| Some(2 * n))
        .find(|x| (x / glyph_width) * (x / glyph_height) >= glyph_count)
        .unwrap();

    let glyphs_per_row = texture_width / glyph_width;
    let rows_used = (glyph_count + glyphs_per_row - 1) / glyphs_per_row;

    let texture_height = std::iter::successors(Some(texture_width), |n| Some(n / 2))
        .find(|n| ((n / 2) + glyph_height - 1) / glyph_height < rows_used)
        .unwrap();

    let glyphs_per_line = texture_width / glyph_width;

    let mut data = vec![0; texture_width as usize * texture_height as usize];

    let origin_x = 0i32;
    let origin_y = ascent as i32;

    for (i, glyph) in font.glyphs().values().enumerate() {
        let i = i as u32;
        let tile_x = i % glyphs_per_line;
        let tile_y = i / glyphs_per_line;

        let anchor_x = origin_x + glyph.bounds().x;
        let anchor_y = origin_y - glyph.bounds().y - glyph.bounds().height as i32;

        for ((glyph_x, glyph_y), _) in glyph.pixels().filter(|&(_, v)| v) {
            let x = anchor_x + glyph_x as i32;
            let y = anchor_y + glyph_y as i32;

            let pixel_x = glyph_width * tile_x + x as u32;
            let pixel_y = glyph_height * tile_y + y as u32;
            let pixel_index = pixel_x as usize + (texture_width * pixel_y) as usize;

            data[pixel_index] = u8::MAX;
        }
    }

    for (i, &px) in data.iter().enumerate() {
        if i % texture_width as usize == 0 {
            println!();
        }

        if px != 0 {
            print!("#");
        } else {
            print!(".");
        }
    }
}
