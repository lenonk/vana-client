//! The Vulkan device with what a neural rendering engine needs (config `neural`, external.rs): wgpu's
//! SHADER_F16, SHADER_I16 and EXPERIMENTAL_COOPERATIVE_MATRIX make wgpu-hal enable shaderFloat16 / shaderInt8,
//! shaderInt16, 16-bit storage, cooperativeMatrix and the Vulkan memory model with device scope (and
//! subgroupSizeControl on 1.3); the rest goes on the create info's
//! pNext chain through wgpu-hal's open_with_callback, in struct types wgpu-hal never chains itself. Used only when
//! the engine library is there; any required feature missing leaves the usual device and says why.

use ash::vk;
use std::ffi::{CStr, c_void};

/// VK_KHR_pipeline_binary is newer than the ash in the tree: its feature struct by hand.
const PIPELINE_BINARY: &CStr = c"VK_KHR_pipeline_binary";
const PIPELINE_BINARY_FEATURES: i32 = 1_000_483_000;

#[repr(C)]
struct PipelineBinaryFeatures {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    pipeline_binaries: vk::Bool32,
}

/// The engine's optional features this device has.
#[derive(Default, Debug, Clone, Copy)]
struct Optional {
    workgroup_layout: bool,
    robustness: bool,
    binaries: bool,
}

/// What the device supports of the engine's list: Err(the missing required ones), else the optional ones there.
fn check(adapter: &wgpu::hal::vulkan::Adapter, wgpu_features: wgpu::Features) -> Result<Optional, String> {
    let instance = adapter.shared_instance().raw_instance();
    let pd = adapter.raw_physical_device();
    // Safety: plain queries on the adapter's own instance and physical device.
    let exts = unsafe { instance.enumerate_device_extension_properties(pd) }.map_err(|e| format!("{e}"))?;
    let has = |n: &CStr| exts.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|x| x == n));
    let mut s8 = vk::PhysicalDevice8BitStorageFeatures::default();
    let mut s16 = vk::PhysicalDevice16BitStorageFeatures::default();
    let mut f16 = vk::PhysicalDeviceShaderFloat16Int8Features::default();
    let mut mm = vk::PhysicalDeviceVulkanMemoryModelFeatures::default();
    let mut ssc = vk::PhysicalDeviceSubgroupSizeControlFeatures::default();
    let mut sync2 = vk::PhysicalDeviceSynchronization2Features::default();
    let mut coop = vk::PhysicalDeviceCooperativeMatrixFeaturesKHR::default();
    let mut wml = vk::PhysicalDeviceWorkgroupMemoryExplicitLayoutFeaturesKHR::default();
    let mut rob = vk::PhysicalDevicePipelineRobustnessFeaturesEXT::default();
    let mut bin = PipelineBinaryFeatures {
        s_type: vk::StructureType::from_raw(PIPELINE_BINARY_FEATURES),
        p_next: std::ptr::null_mut(),
        pipeline_binaries: 0,
    };
    let coop_ext = has(ash::khr::cooperative_matrix::NAME);
    let wml_ext = has(ash::khr::workgroup_memory_explicit_layout::NAME);
    let rob_ext = has(ash::ext::pipeline_robustness::NAME);
    let bin_ext = has(PIPELINE_BINARY) && has(ash::khr::maintenance5::NAME);
    let mut f2 = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut s8)
        .push_next(&mut s16)
        .push_next(&mut f16)
        .push_next(&mut mm)
        .push_next(&mut ssc)
        .push_next(&mut sync2);
    if coop_ext {
        f2 = f2.push_next(&mut coop);
    }
    if wml_ext {
        f2 = f2.push_next(&mut wml);
    }
    if rob_ext {
        f2 = f2.push_next(&mut rob);
    }
    if bin_ext {
        bin.p_next = f2.p_next;
        f2.p_next = (&mut bin as *mut PipelineBinaryFeatures).cast();
    }
    let mut ssp = vk::PhysicalDeviceSubgroupSizeControlProperties::default();
    let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut ssp);
    // Safety: the chains point at the locals above, alive across both calls.
    unsafe {
        instance.get_physical_device_features2(pd, &mut f2);
        instance.get_physical_device_properties2(pd, &mut p2);
    }
    let core = f2.features;
    let api = p2.properties.api_version;
    let lanes32 = ssp.required_subgroup_size_stages.contains(vk::ShaderStageFlags::COMPUTE)
        && ssp.min_subgroup_size <= 32
        && ssp.max_subgroup_size >= 32;
    log::info!(
        "neural: Vulkan {}.{}, compute subgroups {}..{} (required size in compute: {}), 32 lanes {}",
        vk::api_version_major(api),
        vk::api_version_minor(api),
        ssp.min_subgroup_size,
        ssp.max_subgroup_size,
        ssp.required_subgroup_size_stages.contains(vk::ShaderStageFlags::COMPUTE),
        if lanes32 { "yes" } else { "no" },
    );
    let on = |b: vk::Bool32| b == vk::TRUE;
    let mut missing = Vec::new();
    for (ok, name) in [
        (api >= vk::API_VERSION_1_3, "Vulkan 1.3"),
        (coop_ext && on(coop.cooperative_matrix), "cooperativeMatrix"),
        (on(s16.storage_buffer16_bit_access), "storageBuffer16BitAccess"),
        (on(s8.storage_buffer8_bit_access), "storageBuffer8BitAccess"),
        (on(f16.shader_float16), "shaderFloat16"),
        (on(f16.shader_int8), "shaderInt8"),
        (on(mm.vulkan_memory_model), "vulkanMemoryModel"),
        (on(mm.vulkan_memory_model_device_scope), "vulkanMemoryModelDeviceScope"),
        (on(core.shader_int16), "shaderInt16"),
        (on(ssc.subgroup_size_control), "subgroupSizeControl"),
        (on(sync2.synchronization2), "synchronization2"),
        (wgpu_features.contains(wgpu::Features::SHADER_F16), "wgpu SHADER_F16"),
        (wgpu_features.contains(wgpu::Features::SHADER_I16), "wgpu SHADER_I16"),
        (wgpu_features.contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX), "wgpu cooperative matrix"),
    ] {
        if !ok {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        return Err(format!("the device lacks {}", missing.join(", ")));
    }
    Ok(Optional {
        workgroup_layout: wml_ext
            && on(wml.workgroup_memory_explicit_layout)
            && on(wml.workgroup_memory_explicit_layout8_bit_access)
            && on(wml.workgroup_memory_explicit_layout16_bit_access),
        robustness: rob_ext && on(rob.pipeline_robustness),
        binaries: bin_ext && on(bin.pipeline_binaries),
    })
}

/// The device and queue for `desc`, with the engine's features added; Err(why not) when the device lacks a required
/// one (the caller then makes the usual device).
pub fn open(adapter: &wgpu::Adapter, desc: &wgpu::DeviceDescriptor) -> Result<(wgpu::Device, wgpu::Queue), String> {
    use wgpu::hal::api::Vulkan;
    let features = desc.required_features
        | wgpu::Features::SHADER_F16
        | wgpu::Features::SHADER_I16
        | wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX;
    // Safety: the hal adapter is only queried and opened; wgpu takes the device back below.
    let hal = unsafe { adapter.as_hal::<Vulkan>() }.ok_or("not a Vulkan adapter")?;
    let opt = check(&hal, adapter.features())?;
    let mut s8 = vk::PhysicalDevice8BitStorageFeatures::default().storage_buffer8_bit_access(true);
    let mut sync2 = vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true);
    let mut wml = vk::PhysicalDeviceWorkgroupMemoryExplicitLayoutFeaturesKHR::default()
        .workgroup_memory_explicit_layout(true)
        .workgroup_memory_explicit_layout8_bit_access(true)
        .workgroup_memory_explicit_layout16_bit_access(true);
    let mut rob = vk::PhysicalDevicePipelineRobustnessFeaturesEXT::default().pipeline_robustness(true);
    let mut m5 = vk::PhysicalDeviceMaintenance5FeaturesKHR::default().maintenance5(true);
    let mut bin = PipelineBinaryFeatures {
        s_type: vk::StructureType::from_raw(PIPELINE_BINARY_FEATURES),
        p_next: std::ptr::null_mut(),
        pipeline_binaries: vk::TRUE,
    };
    let callback: Box<wgpu::hal::vulkan::CreateDeviceCallback<'_>> = Box::new(|args| {
        let info = std::mem::take(args.create_info);
        let mut info = info.push_next(&mut s8).push_next(&mut sync2);
        if opt.workgroup_layout {
            args.extensions.push(ash::khr::workgroup_memory_explicit_layout::NAME);
            info = info.push_next(&mut wml);
        }
        if opt.robustness {
            args.extensions.push(ash::ext::pipeline_robustness::NAME);
            info = info.push_next(&mut rob);
        }
        if opt.binaries {
            args.extensions.push(PIPELINE_BINARY);
            args.extensions.push(ash::khr::maintenance5::NAME);
            info = info.push_next(&mut m5);
            bin.p_next = info.p_next as *mut c_void;
            info.p_next = (&mut bin as *mut PipelineBinaryFeatures).cast();
        }
        *args.create_info = info;
    });
    // Safety: the callback only adds extensions and feature structs the device was found to support.
    let open = unsafe { hal.open_with_callback(features, &desc.required_limits, &desc.memory_hints, Some(callback)) }
        .map_err(|e| format!("vkCreateDevice: {e}"))?;
    drop(hal);
    let desc = wgpu::DeviceDescriptor {
        required_features: features,
        // Safety: cooperative matrices are only used by the engine, through its own Vulkan calls.
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..desc.clone()
    };
    // Safety: `open` came from this adapter's hal adapter with these features.
    let made = unsafe { adapter.create_device_from_hal(open, &desc) }.map_err(|e| format!("{e}"))?;
    log::info!(
        "neural: device opened with the engine's features (workgroup layout {}, pipeline robustness {}, pipeline binaries {})",
        opt.workgroup_layout,
        opt.robustness,
        opt.binaries
    );
    Ok(made)
}
