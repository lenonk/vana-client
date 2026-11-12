//! The client's process for mods (`process` permission): CPU affinity and scheduling priority, for every thread.

use super::api::need;
use mlua::{Lua, Table};

/// Priority levels, lowest first, with the Unix nice value each is.
const LEVELS: [(&str, i32); 6] =
    [("idle", 19), ("low", 10), ("below_normal", 5), ("normal", 0), ("above_normal", -5), ("high", -10)];

#[cfg(target_os = "linux")]
fn tasks() -> Vec<i32> {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect()
}

/// Every thread on these cores (Linux: each task; threads made later inherit it).
#[cfg(target_os = "linux")]
fn set_affinity(cores: &[usize]) -> Result<(), String> {
    // SAFETY: a zeroed cpu_set_t is the empty set; the libc macros only write inside it.
    if let Some(c) = cores.iter().find(|&&c| c >= libc::CPU_SETSIZE as usize) {
        return Err(format!("no core {c} (cores count from 0)"));
    }
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for &c in cores {
        unsafe { libc::CPU_SET(c, &mut set) };
    }
    for tid in tasks() {
        if unsafe { libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) } != 0 {
            return Err(format!("sched_setaffinity: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn affinity() -> Vec<usize> {
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) } != 0 {
        return Vec::new();
    }
    (0..libc::CPU_SETSIZE as usize).filter(|&c| unsafe { libc::CPU_ISSET(c, &set) }).collect()
}

#[cfg(target_os = "linux")]
fn set_priority(nice: i32) -> Result<(), String> {
    for tid in tasks() {
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice) } != 0 {
            return Err(format!("setpriority: {} (raising it needs privileges)", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn priority() -> i32 {
    unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) }
}

#[cfg(windows)]
fn set_affinity(cores: &[usize]) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};
    let mask = cores.iter().filter(|&&c| c < usize::BITS as usize).fold(0usize, |m, &c| m | 1 << c);
    if unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) } == 0 {
        Err(format!("SetProcessAffinityMask: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn affinity() -> Vec<usize> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask};
    let (mut process, mut system) = (0usize, 0usize);
    if unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut process, &mut system) } == 0 {
        return Vec::new();
    }
    (0..usize::BITS as usize).filter(|c| process >> c & 1 != 0).collect()
}

/// The computer's cores, whatever the affinity (available_parallelism follows it on Linux).
fn cores() -> usize {
    #[cfg(unix)]
    {
        let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        if n > 0 {
            return n as usize;
        }
    }
    #[cfg(windows)]
    {
        // ALL_PROCESSOR_GROUPS
        let n = unsafe { windows_sys::Win32::System::Threading::GetActiveProcessorCount(0xffff) };
        if n > 0 {
            return n as usize;
        }
    }
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

#[cfg(windows)]
fn set_priority(nice: i32) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::*;
    let class = match nice {
        n if n >= 19 => IDLE_PRIORITY_CLASS,
        n if n > 0 => BELOW_NORMAL_PRIORITY_CLASS,
        0 => NORMAL_PRIORITY_CLASS,
        n if n > -10 => ABOVE_NORMAL_PRIORITY_CLASS,
        _ => HIGH_PRIORITY_CLASS,
    };
    if unsafe { SetPriorityClass(GetCurrentProcess(), class) } == 0 {
        Err(format!("SetPriorityClass: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn priority() -> i32 {
    use windows_sys::Win32::System::Threading::*;
    match unsafe { GetPriorityClass(GetCurrentProcess()) } {
        IDLE_PRIORITY_CLASS => 19,
        BELOW_NORMAL_PRIORITY_CLASS => 5,
        ABOVE_NORMAL_PRIORITY_CLASS => -5,
        HIGH_PRIORITY_CLASS | REALTIME_PRIORITY_CLASS => -10,
        _ => 0,
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn set_affinity(_: &[usize]) -> Result<(), String> {
    Err("CPU affinity is not available on this system".into())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn affinity() -> Vec<usize> {
    Vec::new()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn set_priority(nice: i32) -> Result<(), String> {
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } != 0 {
        Err(format!("setpriority: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn priority() -> i32 {
    unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) }
}

/// The nearest level to a nice value.
fn level_of(nice: i32) -> &'static str {
    LEVELS.iter().min_by_key(|l| (l.1 - nice).abs()).map_or("normal", |l| l.0)
}

pub(super) fn install(lua: &Lua, client: &Table, mod_id: &str) -> mlua::Result<()> {
    let me = mod_id.to_string();
    client.set(
        "record",
        lua.create_function(move |lua, (name, opts): (Option<String>, Option<Table>)| {
            need(lua, "exec")?;
            let fps = opts.map(|o| o.get::<Option<u32>>("fps")).transpose()?.flatten().unwrap_or(30).clamp(1, 120);
            let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
            let name = name.unwrap_or(stamp).replace(['/', '\\'], "_");
            // Another mod's recording is not ours to end.
            if let Some((owner, _)) =
                lua.app_data_ref::<super::Recording>().and_then(|r| r.0.borrow().clone()).filter(|r| r.0 != me)
            {
                return Err(mlua::Error::runtime(format!("{owner} is recording")));
            }
            // A new name for each file: an earlier recording is never replaced.
            let dir = crate::config::data_dir().join("ffxiuser").join("videos");
            let mut path = dir.join(format!("{name}.mp4"));
            for n in 2.. {
                if !path.exists() {
                    break;
                }
                path = dir.join(format!("{name}-{n}.mp4"));
            }
            super::api::push_op(lua, super::Op::Record(me.clone(), Some((path.clone(), fps))))?;
            Ok(path.display().to_string())
        })?,
    )?;
    let me2 = mod_id.to_string();
    client.set(
        "stop_recording",
        lua.create_function(move |lua, ()| {
            super::api::push_op(lua, super::Op::Record(me2.clone(), None))?;
            Ok(())
        })?,
    )?;
    client.set(
        "recording",
        lua.create_function(|lua, ()| {
            Ok(lua
                .app_data_ref::<super::Recording>()
                .and_then(|r| r.0.borrow().as_ref().map(|p| p.1.display().to_string())))
        })?,
    )?;
    client.set(
        "process",
        lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            t.set("pid", std::process::id())?;
            t.set("cores", cores())?;
            t.set("affinity", affinity())?;
            t.set("priority", level_of(priority()))?;
            Ok(t)
        })?,
    )?;
    client.set(
        "set_affinity",
        lua.create_function(|lua, cores: Vec<usize>| {
            need(lua, "process")?;
            if cores.is_empty() {
                return Err(mlua::Error::runtime("set_affinity needs at least one core (from 0)"));
            }
            Ok(match set_affinity(&cores) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            })
        })?,
    )?;
    client.set(
        "set_priority",
        lua.create_function(|lua, level: String| {
            need(lua, "process")?;
            let Some(&(_, nice)) = LEVELS.iter().find(|l| l.0 == level) else {
                let names: Vec<&str> = LEVELS.iter().map(|l| l.0).collect();
                return Err(mlua::Error::runtime(format!("priority: {}", names.join(", "))));
            };
            Ok(match set_priority(nice) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            })
        })?,
    )?;
    Ok(())
}
