use ash::vk;
use std::{
    ffi::{c_void, CStr},
    sync::Arc,
};

pub struct Instance {
    pub entry: ash::Entry,
    pub raw: ash::Instance,
    debug_utils: Option<ash::extensions::ext::DebugUtils>,
    messenger: Option<vk::DebugUtilsMessengerEXT>,
}

impl Instance {
    pub fn builder() -> InstanceBuilder {
        InstanceBuilder::default()
    }

    fn create(builder: InstanceBuilder) -> anyhow::Result<Self> {
        let entry = unsafe { ash::Entry::load()? };

        match entry.try_enumerate_instance_version()? {
            Some(version) => {
                log::info!(
                    "Vulkan version: {}.{}.{}",
                    vk::api_version_major(version),
                    vk::api_version_minor(version),
                    vk::api_version_patch(version),
                );
            }
            None => log::info!("Vulkan version: 1.0"),
        };

        let app_info = vk::ApplicationInfo::builder().api_version(vk::make_api_version(0, 1, 2, 0));

        let mut extensions: Vec<_> = builder.extensions.into_iter().map(CStr::as_ptr).collect();
        let mut layers = vec![];

        if builder.debug {
            extensions.push(vk::ExtDebugUtilsFn::name().as_ptr());
            layers.push(super::LAYER_KHRONOS_VALIDATION_LAYER_NAME);
        }

        let instance_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_extension_names(&extensions)
            .enabled_layer_names(&layers);

        let instance = unsafe { entry.create_instance(&instance_info, None)? };

        let (debug_utils, messenger) = if builder.debug {
            let messenger_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(vulkan_debug_callback));

            let debug_utils = ash::extensions::ext::DebugUtils::new(&entry, &instance);
            let messenger =
                unsafe { debug_utils.create_debug_utils_messenger(&messenger_info, None) };
            (Some(debug_utils), messenger.ok())
        } else {
            (None, None)
        };

        Ok(Self {
            entry,
            raw: instance,
            debug_utils,
            messenger,
        })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(debug_utils) = self.debug_utils.take() {
            let messenger = self.messenger.take().unwrap();
            unsafe { debug_utils.destroy_debug_utils_messenger(messenger, None) };
        }
    }
}

#[derive(Default)]
pub struct InstanceBuilder {
    extensions: Vec<&'static CStr>,
    debug: bool,
}

impl InstanceBuilder {
    pub fn build(self) -> anyhow::Result<Arc<Instance>> {
        Instance::create(self).map(Arc::new)
    }

    pub fn extensions(mut self, extensions: Vec<&'static CStr>) -> Self {
        self.extensions = extensions;
        self
    }

    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }
}

unsafe extern "system" fn vulkan_debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    let msg = CStr::from_ptr((*callback_data).p_message).to_string_lossy();
    if msg.starts_with("Device Extension: ") {
        return vk::FALSE;
    }
    let kind = match _message_type {
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL => "general",
        vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE => "performance",
        vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION => "validation",
        _ => unreachable!(),
    };
    match severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => log::error!("(vk: {}) {}", kind, msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => log::warn!("(vk: {}) {}", kind, msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => log::info!("(vk: {}) {}", kind, msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => log::trace!("(vk: {}) {}", kind, msg),
        _ => unreachable!(),
    }
    vk::FALSE
}
