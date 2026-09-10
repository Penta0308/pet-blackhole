#![allow(unsafe_op_in_unsafe_fn)]

use anyhow::{Context, bail};
use ash::{Entry, vk};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::core::PetRenderSnapshot;

const MAX_FRAMES_IN_FLIGHT: usize = 2;
const VERT_SPV: &[u8] = include_bytes!("../../target/slime.vert.spv");
const FRAG_SPV: &[u8] = include_bytes!("../../target/slime.frag.spv");

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    time: f32,
    opacity: f32,
    residue: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ShapeBufferData {
    points: [[f32; 2]; 24],
}

struct MappedBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut ShapeBufferData,
}

pub struct VulkanRenderer {
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    queue_family: u32,
    graphics_queue: vk::Queue,
    swapchain_loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    swapchain_format: vk::Format,
    extent: vk::Extent2D,
    images: Vec<vk::Image>,
    image_views: Vec<vk::ImageView>,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    framebuffers: Vec<vk::Framebuffer>,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_sets: Vec<vk::DescriptorSet>,
    shape_buffers: Vec<MappedBuffer>,
    image_available: Vec<vk::Semaphore>,
    render_finished: Vec<vk::Semaphore>,
    in_flight: Vec<vk::Fence>,
    frame: usize,
    resized: bool,
}

impl VulkanRenderer {
    pub fn new(window: &Window) -> anyhow::Result<Self> {
        let entry = unsafe { Entry::load() }.context("load Vulkan loader")?;

        let app_name = c"pet-blackhole";
        let engine_name = c"pet-slime-core";
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(engine_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_2);

        let display_handle = window.display_handle().context("get display handle")?;
        let window_handle = window.window_handle().context("get window handle")?;
        let required_extensions =
            ash_window::enumerate_required_extensions(display_handle.as_raw())
                .context("enumerate required Vulkan window extensions")?;

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(required_extensions);

        let instance = unsafe { entry.create_instance(&create_info, None) }
            .context("create Vulkan instance")?;
        let surface = unsafe {
            ash_window::create_surface(
                &entry,
                &instance,
                display_handle.as_raw(),
                window_handle.as_raw(),
                None,
            )
        }
        .context("create Vulkan surface")?;
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

        let (physical_device, queue_family) = pick_device(&instance, &surface_loader, surface)?;
        let priorities = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_extensions);
        let device = unsafe { instance.create_device(physical_device, &device_info, None) }
            .context("create Vulkan device")?;
        let graphics_queue = unsafe { device.get_device_queue(queue_family, 0) };
        let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);

        let mut renderer = Self {
            instance,
            surface_loader,
            surface,
            physical_device,
            device,
            queue_family,
            graphics_queue,
            swapchain_loader,
            swapchain: vk::SwapchainKHR::null(),
            swapchain_format: vk::Format::UNDEFINED,
            extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
            images: Vec::new(),
            image_views: Vec::new(),
            render_pass: vk::RenderPass::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            framebuffers: Vec::new(),
            command_pool: vk::CommandPool::null(),
            command_buffers: Vec::new(),
            descriptor_set_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor_sets: Vec::new(),
            shape_buffers: Vec::new(),
            image_available: Vec::new(),
            render_finished: Vec::new(),
            in_flight: Vec::new(),
            frame: 0,
            resized: false,
        };

        renderer
            .create_swapchain_resources(window.inner_size().width, window.inner_size().height)?;
        log::info!("Vulkan renderer ready");
        Ok(renderer)
    }

    pub fn request_resize(&mut self) {
        self.resized = true;
    }

    pub fn wait_idle(&self) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }
    }

    pub fn render(&mut self, snapshot: &PetRenderSnapshot) -> anyhow::Result<()> {
        if self.extent.width == 0 || self.extent.height == 0 {
            return Ok(());
        }

        unsafe {
            self.device
                .wait_for_fences(&[self.in_flight[self.frame]], true, u64::MAX)?;
            let acquire = self.swapchain_loader.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available[self.frame],
                vk::Fence::null(),
            );
            let image_index = match acquire {
                Ok((index, suboptimal)) => {
                    if suboptimal {
                        self.resized = true;
                    }
                    index
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.resized = true;
                    return Ok(());
                }
                Err(error) => return Err(error).context("acquire next swapchain image"),
            };

            self.device.reset_fences(&[self.in_flight[self.frame]])?;
            self.update_shape_buffer(snapshot);
            self.device.reset_command_buffer(
                self.command_buffers[self.frame],
                vk::CommandBufferResetFlags::empty(),
            )?;
            self.record_command_buffer(
                self.command_buffers[self.frame],
                image_index as usize,
                snapshot,
            )?;

            let wait_semaphores = [self.image_available[self.frame]];
            let signal_semaphores = [self.render_finished[self.frame]];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let command_buffers = [self.command_buffers[self.frame]];
            let submit_info = [vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores)];

            self.device.queue_submit(
                self.graphics_queue,
                &submit_info,
                self.in_flight[self.frame],
            )?;

            let swapchains = [self.swapchain];
            let image_indices = [image_index];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);
            match self
                .swapchain_loader
                .queue_present(self.graphics_queue, &present_info)
            {
                Ok(suboptimal) => {
                    if suboptimal {
                        self.resized = true;
                    }
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.resized = true,
                Err(error) => return Err(error).context("present swapchain image"),
            }

            self.frame = (self.frame + 1) % MAX_FRAMES_IN_FLIGHT;
        }

        Ok(())
    }

    fn update_shape_buffer(&mut self, snapshot: &PetRenderSnapshot) {
        unsafe {
            (*self.shape_buffers[self.frame].mapped).points = snapshot.points;
        }
    }

    unsafe fn record_command_buffer(
        &self,
        cmd: vk::CommandBuffer,
        image_index: usize,
        snapshot: &PetRenderSnapshot,
    ) -> anyhow::Result<()> {
        let begin = vk::CommandBufferBeginInfo::default();
        self.device.begin_command_buffer(cmd, &begin)?;

        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            },
        }];
        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: self.extent,
        };
        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[image_index])
            .render_area(render_area)
            .clear_values(&clear_values);
        self.device
            .cmd_begin_render_pass(cmd, &render_pass_info, vk::SubpassContents::INLINE);
        self.device
            .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        self.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout,
            0,
            &[self.descriptor_sets[self.frame]],
            &[],
        );

        let pc = PushConstants {
            time: snapshot.time * snapshot.wobble.max(0.05),
            opacity: snapshot.opacity,
            residue: snapshot.residue,
            _pad: 0.0,
        };
        let pc_bytes = std::slice::from_raw_parts(
            (&pc as *const PushConstants).cast::<u8>(),
            std::mem::size_of::<PushConstants>(),
        );
        self.device.cmd_push_constants(
            cmd,
            self.pipeline_layout,
            vk::ShaderStageFlags::FRAGMENT,
            0,
            pc_bytes,
        );
        self.device.cmd_draw(cmd, 3, 1, 0, 0);
        self.device.cmd_end_render_pass(cmd);
        self.device.end_command_buffer(cmd)?;
        Ok(())
    }

    fn create_swapchain_resources(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        let caps = unsafe {
            self.surface_loader
                .get_physical_device_surface_capabilities(self.physical_device, self.surface)
        }?;
        let formats = unsafe {
            self.surface_loader
                .get_physical_device_surface_formats(self.physical_device, self.surface)
        }?;
        let present_modes = unsafe {
            self.surface_loader
                .get_physical_device_surface_present_modes(self.physical_device, self.surface)
        }?;

        let surface_format = formats
            .iter()
            .copied()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_UNORM || f.format == vk::Format::R8G8B8A8_UNORM
            })
            .unwrap_or_else(|| formats[0]);
        let present_mode = present_modes
            .iter()
            .copied()
            .find(|m| *m == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);
        let extent = choose_extent(caps, width, height);
        let mut image_count = caps.min_image_count + 1;
        if caps.max_image_count > 0 && image_count > caps.max_image_count {
            image_count = caps.max_image_count;
        }
        let composite_alpha = choose_composite_alpha(caps.supported_composite_alpha);

        let swapchain_info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(composite_alpha)
            .present_mode(present_mode)
            .clipped(true);

        self.swapchain = unsafe {
            self.swapchain_loader
                .create_swapchain(&swapchain_info, None)
        }
        .context("create swapchain")?;
        self.images = unsafe { self.swapchain_loader.get_swapchain_images(self.swapchain) }
            .context("get swapchain images")?;
        self.swapchain_format = surface_format.format;
        self.extent = extent;

        self.image_views = self
            .images
            .iter()
            .map(|&image| unsafe { create_image_view(&self.device, image, self.swapchain_format) })
            .collect::<Result<_, _>>()?;
        self.render_pass = unsafe { create_render_pass(&self.device, self.swapchain_format) }?;
        self.descriptor_set_layout = unsafe { create_descriptor_set_layout(&self.device) }?;
        self.shape_buffers =
            unsafe { create_shape_buffers(&self.instance, &self.device, self.physical_device) }?;
        let descriptor = unsafe {
            create_descriptors(
                &self.device,
                self.descriptor_set_layout,
                &self.shape_buffers,
            )
        }?;
        self.descriptor_pool = descriptor.0;
        self.descriptor_sets = descriptor.1;
        let (layout, pipeline) = unsafe {
            create_pipeline(
                &self.device,
                self.render_pass,
                extent,
                self.descriptor_set_layout,
            )
        }?;
        self.pipeline_layout = layout;
        self.pipeline = pipeline;
        self.framebuffers = self
            .image_views
            .iter()
            .map(|&view| unsafe {
                create_framebuffer(&self.device, self.render_pass, view, extent)
            })
            .collect::<Result<_, _>>()?;
        self.command_pool = unsafe { create_command_pool(&self.device, self.queue_family) }?;
        self.command_buffers =
            unsafe { allocate_command_buffers(&self.device, self.command_pool) }?;
        let sync = unsafe { create_sync(&self.device) }?;
        self.image_available = sync.0;
        self.render_finished = sync.1;
        self.in_flight = sync.2;
        Ok(())
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for &fence in &self.in_flight {
                self.device.destroy_fence(fence, None);
            }
            for &semaphore in &self.render_finished {
                self.device.destroy_semaphore(semaphore, None);
            }
            for &semaphore in &self.image_available {
                self.device.destroy_semaphore(semaphore, None);
            }
            if self.command_pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.command_pool, None);
            }
            for &fb in &self.framebuffers {
                self.device.destroy_framebuffer(fb, None);
            }
            if self.pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(self.pipeline, None);
            }
            if self.pipeline_layout != vk::PipelineLayout::null() {
                self.device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
            }
            if self.descriptor_pool != vk::DescriptorPool::null() {
                self.device
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.descriptor_set_layout != vk::DescriptorSetLayout::null() {
                self.device
                    .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            }
            for buffer in &self.shape_buffers {
                self.device.unmap_memory(buffer.memory);
                self.device.destroy_buffer(buffer.buffer, None);
                self.device.free_memory(buffer.memory, None);
            }
            if self.render_pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(self.render_pass, None);
            }
            for &view in &self.image_views {
                self.device.destroy_image_view(view, None);
            }
            if self.swapchain != vk::SwapchainKHR::null() {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
            }
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

fn pick_device(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> anyhow::Result<(vk::PhysicalDevice, u32)> {
    let devices = unsafe { instance.enumerate_physical_devices() }
        .context("enumerate Vulkan physical devices")?;
    for device in devices {
        let families = unsafe { instance.get_physical_device_queue_family_properties(device) };
        for (index, family) in families.iter().enumerate() {
            let supports_graphics = family.queue_flags.contains(vk::QueueFlags::GRAPHICS);
            let supports_surface = unsafe {
                surface_loader.get_physical_device_surface_support(device, index as u32, surface)
            }
            .unwrap_or(false);
            if supports_graphics && supports_surface {
                return Ok((device, index as u32));
            }
        }
    }
    bail!("no Vulkan device supports graphics + present for this window")
}

fn choose_extent(caps: vk::SurfaceCapabilitiesKHR, width: u32, height: u32) -> vk::Extent2D {
    if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D {
            width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
            height: height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
        }
    }
}

fn choose_composite_alpha(flags: vk::CompositeAlphaFlagsKHR) -> vk::CompositeAlphaFlagsKHR {
    [
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
        vk::CompositeAlphaFlagsKHR::OPAQUE,
    ]
    .into_iter()
    .find(|flag| flags.contains(*flag))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}

unsafe fn create_image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
) -> anyhow::Result<vk::ImageView> {
    let subresource = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(subresource);
    Ok(device.create_image_view(&info, None)?)
}

unsafe fn create_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> anyhow::Result<vk::RenderPass> {
    let attachment = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
    let color_ref = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpass = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref)];
    let dep = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachment)
        .subpasses(&subpass)
        .dependencies(&dep);
    Ok(device.create_render_pass(&info, None)?)
}

unsafe fn create_descriptor_set_layout(
    device: &ash::Device,
) -> anyhow::Result<vk::DescriptorSetLayout> {
    let bindings = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    Ok(device.create_descriptor_set_layout(&info, None)?)
}

unsafe fn create_shape_buffers(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
) -> anyhow::Result<Vec<MappedBuffer>> {
    let mut buffers = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    let size = std::mem::size_of::<ShapeBufferData>() as vk::DeviceSize;
    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = device.create_buffer(&info, None)?;
        let req = device.get_buffer_memory_requirements(buffer);
        let mem_type = find_memory_type(
            instance,
            physical_device,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(mem_type);
        let memory = device.allocate_memory(&alloc, None)?;
        device.bind_buffer_memory(buffer, memory, 0)?;
        let mapped = device
            .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?
            .cast::<ShapeBufferData>();
        buffers.push(MappedBuffer {
            buffer,
            memory,
            mapped,
        });
    }
    Ok(buffers)
}

unsafe fn create_descriptors(
    device: &ash::Device,
    layout: vk::DescriptorSetLayout,
    buffers: &[MappedBuffer],
) -> anyhow::Result<(vk::DescriptorPool, Vec<vk::DescriptorSet>)> {
    let pool_sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(MAX_FRAMES_IN_FLIGHT as u32)];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(MAX_FRAMES_IN_FLIGHT as u32)
        .pool_sizes(&pool_sizes);
    let pool = device.create_descriptor_pool(&pool_info, None)?;
    let layouts = [layout; MAX_FRAMES_IN_FLIGHT];
    let alloc = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    let sets = device.allocate_descriptor_sets(&alloc)?;
    let size = std::mem::size_of::<ShapeBufferData>() as vk::DeviceSize;
    for (set, buffer) in sets.iter().zip(buffers) {
        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(buffer.buffer)
            .offset(0)
            .range(size)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(*set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_info)];
        device.update_descriptor_sets(&writes, &[]);
    }
    Ok((pool, sets))
}

fn find_memory_type(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> anyhow::Result<u32> {
    let mem = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    for i in 0..mem.memory_type_count {
        let supported = (type_bits & (1 << i)) != 0;
        let has_props = mem.memory_types[i as usize]
            .property_flags
            .contains(properties);
        if supported && has_props {
            return Ok(i);
        }
    }
    bail!("no suitable host-visible buffer memory type")
}

unsafe fn create_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    extent: vk::Extent2D,
    descriptor_set_layout: vk::DescriptorSetLayout,
) -> anyhow::Result<(vk::PipelineLayout, vk::Pipeline)> {
    let vert = create_shader_module(device, VERT_SPV)?;
    let frag = create_shader_module(device, FRAG_SPV)?;
    let main = c"main";
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert)
            .name(main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag)
            .name(main),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: extent.width as f32,
        height: extent.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissor = [vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent,
    }];
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewport)
        .scissors(&scissor);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
    let push_range = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        .offset(0)
        .size(std::mem::size_of::<PushConstants>() as u32)];
    let set_layouts = [descriptor_set_layout];
    let layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&set_layouts)
        .push_constant_ranges(&push_range);
    let layout = device.create_pipeline_layout(&layout_info, None)?;
    let info = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0)];
    let pipeline = device
        .create_graphics_pipelines(vk::PipelineCache::null(), &info, None)
        .map_err(|(_, error)| error)
        .context("create graphics pipeline")?[0];
    device.destroy_shader_module(frag, None);
    device.destroy_shader_module(vert, None);
    Ok((layout, pipeline))
}

unsafe fn create_shader_module(
    device: &ash::Device,
    bytes: &[u8],
) -> anyhow::Result<vk::ShaderModule> {
    let words = ash::util::read_spv(&mut std::io::Cursor::new(bytes)).context("read SPIR-V")?;
    let info = vk::ShaderModuleCreateInfo::default().code(&words);
    Ok(device.create_shader_module(&info, None)?)
}

unsafe fn create_framebuffer(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    view: vk::ImageView,
    extent: vk::Extent2D,
) -> anyhow::Result<vk::Framebuffer> {
    let attachments = [view];
    let info = vk::FramebufferCreateInfo::default()
        .render_pass(render_pass)
        .attachments(&attachments)
        .width(extent.width)
        .height(extent.height)
        .layers(1);
    Ok(device.create_framebuffer(&info, None)?)
}

unsafe fn create_command_pool(
    device: &ash::Device,
    queue_family: u32,
) -> anyhow::Result<vk::CommandPool> {
    let info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    Ok(device.create_command_pool(&info, None)?)
}

unsafe fn allocate_command_buffers(
    device: &ash::Device,
    pool: vk::CommandPool,
) -> anyhow::Result<Vec<vk::CommandBuffer>> {
    let info = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);
    Ok(device.allocate_command_buffers(&info)?)
}

unsafe fn create_sync(
    device: &ash::Device,
) -> anyhow::Result<(Vec<vk::Semaphore>, Vec<vk::Semaphore>, Vec<vk::Fence>)> {
    let semaphore_info = vk::SemaphoreCreateInfo::default();
    let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
    let mut image_available = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    let mut render_finished = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    let mut in_flight = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        image_available.push(device.create_semaphore(&semaphore_info, None)?);
        render_finished.push(device.create_semaphore(&semaphore_info, None)?);
        in_flight.push(device.create_fence(&fence_info, None)?);
    }
    Ok((image_available, render_finished, in_flight))
}
