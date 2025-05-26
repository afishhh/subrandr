use std::{
    cell::Cell,
    collections::HashMap,
    fmt::Write,
    rc::{Rc, Weak},
};

use wgpu::TextureFormat;

use crate::math::{Point2, Vec2};

/// A texture packer that packs stuff into (potentially multiple) atlas textures.
///
/// Freeing a texture is implemented by tracking the amount of freed space,
/// which is intially wasted, and recreating the whole atlas once too much space is wasted.
/// Note that freed space is *not* added to the free block pool, this could be changed but
/// will still result in fragmentation so a mechanism to occasionally defragment the atlas is
/// necessary anyway.
pub struct TexturePacker {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: TextureFormat,
    textures: HashMap<u32, AtlasTexture>,
    next_texture_id: u32,
    free: Vec<FreeBlock>,
}

#[derive(Debug, Clone, Copy)]
struct FreeBlock {
    texture: u32,
    size: Vec2<u32>,
    position: Point2<u32>,
}

impl FreeBlock {
    fn fits(&self, request: Vec2<u32>) -> bool {
        self.size.x >= request.x && self.size.y >= request.y
    }
}

#[derive(Debug)]
struct AtlasTexture {
    texture: wgpu::Texture,
    allocated: Vec<Weak<AllocatedBlock>>,
    wasted_space: Rc<Cell<u32>>,
}

#[derive(Debug, Clone)]
struct AllocatedBlock {
    texture: Cell<u32>,
    position: Cell<Point2<u32>>,
    size: Vec2<u32>,
    wasted_space: Rc<Cell<u32>>,
}

impl Drop for AllocatedBlock {
    fn drop(&mut self) {
        let size = self.size;
        self.wasted_space
            .set(self.wasted_space.get() + size.x * size.y);
    }
}

#[derive(Debug, Clone)]
pub struct PackedTexture {
    block: Rc<AllocatedBlock>,
}

impl PackedTexture {
    pub fn size(&self) -> Vec2<u32> {
        self.block.size
    }

    pub fn get_texture_region<'a>(
        &self,
        packer: &'a TexturePacker,
    ) -> (&'a wgpu::Texture, Point2<u32>, Vec2<u32>) {
        (
            &packer.textures[&self.block.texture.get()].texture,
            self.block.position.get(),
            self.block.size,
        )
    }
}

impl TexturePacker {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: TextureFormat) -> Self {
        Self {
            device,
            queue,
            format,
            textures: HashMap::new(),
            next_texture_id: 0,
            free: Vec::new(),
        }
    }

    fn desired_atlas_texture_size(&self, min: u32) -> u32 {
        let desired = 2 << (self.textures.len() + 9).min(12);
        let limit = self.device.limits().max_texture_dimension_2d;
        desired.min(limit).max(min)
    }

    fn split_free(&mut self, free: &FreeBlock, size: Vec2<u32>) {
        let lower_size = Vec2::new(free.size.x, free.size.y - size.y);
        if lower_size.x != 0 && lower_size.y != 0 {
            self.free.push(FreeBlock {
                texture: free.texture,
                size: lower_size,
                position: free.position + Vec2::new(0, size.y),
            });
        }

        let right_size = Vec2::new(free.size.x - size.x, size.y);
        if right_size.x != 0 && right_size.y != 0 {
            self.free.push(FreeBlock {
                texture: free.texture,
                size: right_size,
                position: free.position + Vec2::new(size.x, 0),
            });
        }
    }

    fn allocate_block(&mut self, size: Vec2<u32>) -> FreeBlock {
        if let Some(idx) = self.free.iter().rposition(|free| free.fits(size)) {
            let block = self.free.swap_remove(idx);

            self.split_free(&block, size);
            self.free
                .sort_by_key(|free| std::cmp::Reverse(free.size.x * free.size.y));

            block
        } else {
            let new_atlas_size = self.desired_atlas_texture_size(size.x.max(size.y));
            let texture = self.device.create_texture(&wgpu::wgt::TextureDescriptor {
                label: Some(&format!(
                    "{:?} atlas {1}x{1} texture",
                    self.format, new_atlas_size
                )),
                size: wgpu::Extent3d {
                    width: new_atlas_size,
                    height: new_atlas_size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[self.format],
            });

            let texture_id = self.next_texture_id;
            self.next_texture_id += 1;

            let full_block = FreeBlock {
                texture: texture_id,
                size: Vec2::new(new_atlas_size, new_atlas_size),
                position: Point2::ZERO,
            };

            self.split_free(&full_block, size);

            self.textures.insert(
                texture_id,
                AtlasTexture {
                    texture,
                    allocated: Vec::new(),
                    wasted_space: Rc::new(Cell::new(0)),
                },
            );

            full_block
        }
    }

    /// Defragments texture atlases belonging to this packer.
    ///
    /// Note that this may change what textures existing `PackedTexture`s derived from this
    /// packer point to, thus it shall not be called if those are expected not to change, like
    /// when a draw is being actively batched.
    pub fn defragment(&mut self) {
        // TODO: hash_extract_if will be stabilised in 1.88.0
        //       they delayed it from 1.87 :(
        // let fragmented = self
        //     .textures
        //     .extract_if(|_, atlas| {
        //         let total_space = atlas.texture.width() * atlas.texture.height();
        //         // This texture has 1/3 of its space unused, take it out and
        //         // we'll reinsert its components into a new texture.
        //         atlas.wasted_space.get() * 3 >= total_space
        //     })
        //     .map(|(_, atlas)| atlas)
        //     .collect::<Vec<_>>();

        // Written this way to allow replacing with above code immediately after hash_extract_if is
        // stabilised.
        let fragmented = {
            self.textures
                .iter()
                .filter_map(|(&key, atlas)| {
                    let total_space = atlas.texture.width() * atlas.texture.height();
                    // This texture has 1/3 of its space unused, take it out and
                    // we'll reinsert its components into a new texture.
                    if atlas.wasted_space.get() * 3 >= total_space {
                        Some(key)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|id| self.textures.remove(&id).unwrap())
                .collect::<Vec<_>>()
        };

        self.free
            .retain(|free| self.textures.contains_key(&free.texture));

        for atlas in fragmented {
            for weak_block in atlas.allocated {
                if let Some(block) = weak_block.upgrade() {
                    self.relocate_block(&atlas.texture, block, weak_block);
                }
            }
        }
    }

    fn relocate_block(
        &mut self,
        texture: &wgpu::Texture,
        allocated_block: Rc<AllocatedBlock>,
        weak_allocated_block: Weak<AllocatedBlock>,
    ) {
        let size = allocated_block.size;
        let block = self.allocate_block(size);
        let atlas = self.textures.get_mut(&block.texture).unwrap();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("texture texture atlas move encoder"),
            });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: allocated_block.position.get().x,
                    y: allocated_block.position.get().y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &atlas.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: block.position.x,
                    y: block.position.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        allocated_block.texture.set(block.texture);
        allocated_block.position.set(block.position);
        atlas.allocated.push(weak_allocated_block);
    }

    pub fn add_from_buffer(
        &mut self,
        buffer: &wgpu::Buffer,
        stride: u32,
        width: u32,
        height: u32,
    ) -> PackedTexture {
        let size = Vec2::new(width, height);
        let block = self.allocate_block(size);
        let atlas = self.textures.get_mut(&block.texture).unwrap();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("buffer -> texture atlas move encoder"),
            });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &atlas.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: block.position.x,
                    y: block.position.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let allocated = Rc::new(AllocatedBlock {
            position: Cell::new(block.position),
            size,
            texture: Cell::new(block.texture),
            wasted_space: atlas.wasted_space.clone(),
        });

        atlas.allocated.push(Rc::downgrade(&allocated));

        PackedTexture { block: allocated }
    }

    pub(crate) fn write_atlas_stats(&self, writer: &mut dyn Write) -> std::fmt::Result {
        writeln!(writer, "{} atlas textures", self.textures.len())?;

        let bytes_per_pixel = self.format.target_pixel_byte_cost().unwrap();
        writeln!(
            writer,
            "{} total bytes {} wasted bytes {} free bytes",
            self.textures
                .values()
                .map(|atlas| atlas.texture.width() * atlas.texture.height())
                .sum::<u32>()
                * bytes_per_pixel,
            self.textures
                .values()
                .map(|atlas| atlas.wasted_space.get())
                .sum::<u32>()
                * bytes_per_pixel,
            self.free
                .iter()
                .map(|free| free.size.x * free.size.y)
                .sum::<u32>()
                * bytes_per_pixel,
        )?;

        writeln!(
            writer,
            "{} allocated blocks {} free blocks",
            self.textures
                .values()
                .map(|atlas| atlas.allocated.len())
                .sum::<usize>(),
            self.free.len()
        )?;

        Ok(())
    }
}
