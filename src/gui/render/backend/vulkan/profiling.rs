use super::{commands::RecordingCommandBuffer, Buffer, Device};
use ash::vk;
use std::sync::Arc;

const MAX_QUERY_COUNT: usize = 1024;

pub struct ProfilerData {
    query_pool: vk::QueryPool,
    timestamp_period: f64,
    buffer: Buffer,
}

impl ProfilerData {
    pub fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let size = MAX_QUERY_COUNT * 8 * 2;
        let usage = vk::BufferUsageFlags::TRANSFER_DST;
        let memory_flags = vk::MemoryPropertyFlags::HOST_VISIBLE;
        let buffer = device.create_buffer(usage, memory_flags, size as u64)?;

        let pool_info = vk::QueryPoolCreateInfo::builder()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(MAX_QUERY_COUNT as u32 * 2);

        let timestamp_period = device.physical_device.properties.limits.timestamp_period as f64;

        Ok(Self {
            query_pool: unsafe { device.raw.create_query_pool(&pool_info, None) }?,
            timestamp_period,
            buffer,
        })
    }

    pub fn begin_frame(&self, cb: &RecordingCommandBuffer<'_>) {
        unsafe {
            self.buffer.device.raw.cmd_reset_query_pool(
                cb.0.raw,
                self.query_pool,
                0,
                MAX_QUERY_COUNT as u32 * 2,
            );
            self.buffer.device.raw.cmd_write_timestamp(
                cb.0.raw,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                self.query_pool,
                0,
            );
        }
    }

    pub fn finish_frame(&self, cb: &RecordingCommandBuffer<'_>) {
        unsafe {
            self.buffer.device.raw.cmd_write_timestamp(
                cb.0.raw,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                self.query_pool,
                1,
            );
            self.buffer.device.raw.cmd_copy_query_pool_results(
                cb.0.raw,
                self.query_pool,
                0,
                2,
                self.buffer.raw,
                0,
                8,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            );
        }
    }

    pub fn retrieve_previous_result(&self) -> anyhow::Result<std::time::Duration> {
        let mapped_ptr = unsafe {
            self.buffer.device.raw.map_memory(
                self.buffer.memory,
                0,
                2 * 8,
                vk::MemoryMapFlags::empty(),
            )
        }? as *const u64;
        let data = unsafe { std::slice::from_raw_parts(mapped_ptr, 2) };

        let start = data[0];
        let end = data[1];

        unsafe {
            self.buffer.device.raw.unmap_memory(self.buffer.memory);
        }

        let duration = (end - start) as f64 / self.timestamp_period;
        let duration = std::time::Duration::from_nanos(duration as u64);

        Ok(duration)
    }
}

impl Drop for ProfilerData {
    fn drop(&mut self) {
        unsafe {
            self.buffer
                .device
                .raw
                .destroy_query_pool(self.query_pool, None);
        }
    }
}
