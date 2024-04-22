#![cfg_attr(
    target_arch = "spirv",
    feature(register_attr),
    register_attr(spirv),
    no_std
)]

extern crate spirv_std;

use spirv_std::{glam, Image};

#[cfg(not(target_arch = "spirv"))]
use spirv_std::macros::spirv;

// #[spirv(compute(threads(8, 8)))]
// pub fn main_cs(
//     #[spirv(global_invocation_id)] id: glam::UVec3,
//     #[spirv(local_invocation_id)] local_id: glam::UVec3,
//     // #[spirv(descriptor_set = 0, binding = 0)] image: &mut spirv_std::image::StorageImage2d,
//     #[spirv(descriptor_set = 0, binding = 0)] image: &Image!(2D, format=rgba32f, sampled=false),
// ) {
//     let coords = glam::uvec2(id.x, id.y);
//     let color = glam::vec3(local_id.x as f32 / 8.0, 0.0, local_id.y as f32 / 8.0);
//     unsafe {
//         image.write(coords, color);
//     }
// }

#[repr(C)]
pub struct Glyph {
    atlas_x: u32,
    atlas_y: u32,
    pos_x: u32,
    pos_y: u32,
}

#[spirv(compute(threads(5, 11)))]
pub fn cs_with_font(
    #[spirv(workgroup_id)] id: glam::UVec3,
    #[spirv(local_invocation_id)] local_id: glam::UVec3,
    #[spirv(descriptor_set = 0, binding = 0)] fb: &Image!(2D, format=rgba32f, sampled=false),
    #[spirv(descriptor_set = 0, binding = 1)] atlas: &Image!(2D, format=r8ui, sampled=false),
    #[spirv(descriptor_set = 0, binding = 2, storage_buffer)] data: &[Glyph],
) {
    let our_glyph = &data[id.x as usize];
    let atlas_entry = glam::uvec2(our_glyph.atlas_x, our_glyph.atlas_y) * glam::uvec2(5, 11);

    let global_coords = glam::uvec2(our_glyph.pos_x, our_glyph.pos_y) * glam::uvec2(5, 11);
    let local_coords = glam::uvec2(local_id.x, local_id.y);

    let read_coords = atlas_entry + local_coords;
    let write_coords = global_coords + local_coords;

    let atlas_px: glam::UVec4 = atlas.read(read_coords);
    // let color = (atlas_px.x as f32 / 255.0) * glam::vec3(0.0, 0.8, 1.0);
    let color = (atlas_px.x as f32 / 255.0) * glam::vec3(1.0, 1.0, 1.0);
    unsafe {
        fb.write(write_coords, color);
    }
}

// #[spirv(compute(threads(8, 8)))]
// pub fn fb_cs(
//     #[spirv(global_invocation_id)] id: glam::UVec3,
//     #[spirv(local_invocation_id)] local_id: glam::UVec3,
//     #[spirv(push_constant)] fb: &u32,
//     // #[spirv(descriptor_set = 0, binding = 0)] image: &mut spirv_std::image::StorageImage2d,
//     #[spirv(descriptor_set = 0, binding = 0)] image: &[Image!(2D, format=rgba32f, sampled=false);
//          3],
// ) {
//     let coords = glam::uvec2(id.x, id.y);
//     let color = glam::vec3(
//         local_id.x as f32 / 8.0,
//         (*fb as f32) / 3.0,
//         local_id.y as f32 / 8.0,
//     );
//     unsafe {
//         image[*fb as usize].write(coords, color);
//     }
// }

const VERTICES: [[f32; 4]; 6] = [
    [-1.0, -1.0, 0.0, 1.0],
    [-1.0, 1.0, 0.0, 1.0],
    [1.0, 1.0, 0.0, 1.0],
    [1.0, 1.0, 0.0, 1.0],
    [1.0, -1.0, 0.0, 1.0],
    [-1.0, -1.0, 0.0, 1.0],
];

#[spirv(vertex)]
pub fn main_vs(
    #[spirv(vertex_index)] vert_id: i32,
    #[spirv(position, invariant)] out_pos: &mut glam::Vec4,
) {
    // *out_pos = glam::vec4(
    //     (vert_id - 1) as f32,
    //     ((vert_id & 1) * 2 - 1) as f32,
    //     0.0,
    //     1.0,
    // )
    let vertex = VERTICES[vert_id as usize];
    *out_pos = glam::vec4(vertex[0], vertex[1], vertex[2], vertex[3]);
}

#[spirv(fragment)]
pub fn main_fs(
    #[spirv(descriptor_set = 0, binding = 0)] image: &Image!(2D, format=rgba32f, sampled=false),
    #[spirv(frag_coord)] coords: glam::Vec4,
    output: &mut glam::Vec4,
) {
    let coords = glam::uvec2(coords.x as u32, coords.y as u32);
    let out: glam::Vec4 = image.read(coords);
    *output = glam::vec4(out.x, out.y, out.z, 1.0);
    // *output = glam::vec4(0.0, 0.8, 1.0, 1.0);
}
