use std::{
    mem::size_of,
    process::Command,
    ptr::{null, null_mut},
    time::Duration,
};

use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use regex::Regex;
use winapi::{
    shared::minwindef::{BOOL, DWORD, FALSE},
    um::{
        consoleapi::SetConsoleCtrlHandler,
        handleapi::CloseHandle,
        minwinbase::STILL_ACTIVE,
        processthreadsapi::{GetExitCodeProcess, OpenProcess},
        psapi::{EnumProcesses, GetModuleFileNameExW},
        wingdi::{DEVMODEA, DM_PELSHEIGHT, DM_PELSWIDTH},
        winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        winuser::{
            ChangeDisplaySettingsA, EnumDisplaySettingsA, DISP_CHANGE_SUCCESSFUL,
            ENUM_REGISTRY_SETTINGS,
        },
    },
};

#[cfg(not(windows))]
compile_error!("this program can only running on windows system");

fn main() {
    let Err(err) = main_func() else {
        return;
    };

    eprintln!("错误：{err}");
    println!("按回车键继续...");
    std::io::stdin().lines().next();
}

fn main_func() -> Result<()> {
    let mut args = std::env::args();
    let (width, height): (u32, u32) = match (args.nth(1), args.next()) {
        (Some(w), Some(h)) => (w.parse()?, h.parse()?),
        _ => {
            return Err(anyhow!(
                "未提供目标分辨率。\
            提示：您需要在程序末尾添加分辨率。例如：\
            执行`C:\\Users\\FancyFlame\\Desktop\\res_set.exe 1600 1000`\
            将设定目标分辨率为1600x1000。方便起见，您可以创建一个快捷方式来\
            放置此指令。"
            ))
        }
    };

    println!("正在调整分辨率到{width}x{height}...");
    set_resolution(width, height)?;

    println!("正在设置关闭钩子...");
    recover_on_close()?;

    println!("程序已启动");
    Command::new("explorer.exe ")
        .arg(r"shell:AppsFolder\Microsoft.MinecraftUWP_8wekyb3d8bbwe!App")
        .output()
        .map_err(|err| anyhow!("程序异常退出：{err}"))?;

    println!("正在等待目标程序进入检测...");
    let proc_id = loop {
        if let Some(id) = get_proc_id()? {
            break id;
        }
        std::thread::sleep(Duration::from_millis(500));
    };

    println!("检测到程序正在运行。关闭目标程序或此窗口来恢复分辨率。");
    while check_alive(proc_id) {
        std::thread::sleep(Duration::from_secs(1));
    }

    println!("检测到目标程序退出。恢复分辨率。");
    recover_resolution()
}

fn get_origin_resolution() -> Result<(u32, u32)> {
    let mut dev_mode = DEVMODEA {
        dmSize: size_of::<DEVMODEA>() as u16,
        ..Default::default()
    };

    let result = unsafe { EnumDisplaySettingsA(null(), ENUM_REGISTRY_SETTINGS, &mut dev_mode) };

    if result == 0 {
        Err(anyhow!("无法获取原屏幕分辨率"))
    } else {
        Ok((dev_mode.dmPelsWidth, dev_mode.dmPelsHeight))
    }
}

fn set_resolution(width: u32, height: u32) -> Result<()> {
    let mut dev_mode = DEVMODEA {
        dmPelsWidth: width,
        dmPelsHeight: height,
        dmSize: size_of::<DEVMODEA>() as u16,
        dmFields: DM_PELSWIDTH | DM_PELSHEIGHT,
        ..Default::default()
    };

    let result = unsafe { ChangeDisplaySettingsA(&mut dev_mode, 0) };
    if result != DISP_CHANGE_SUCCESSFUL {
        Err(anyhow!("无法设置屏幕分辨率，错误码{result}"))
    } else {
        Ok(())
    }
}

fn recover_resolution() -> Result<()> {
    let (w, h) = get_origin_resolution()?;
    println!("程序已结束，正在调回分辨率到{w}x{h}...");
    set_resolution(w, h)
}

fn get_proc_id() -> Result<Option<u32>> {
    lazy_static! {
        static ref FILE_REGEX: Regex = Regex::new(
            r"Microsoft\.MinecraftUWP_[0-9.]+_\w+__8wekyb3d8bbwe\\Minecraft\.Windows\.exe$"
        )
        .unwrap();
    }

    let file_regex = &*FILE_REGEX;

    const BUF_SIZE: usize = 2048;
    let mut buffer = [0u32; BUF_SIZE];
    let mut size = 0;

    unsafe {
        if EnumProcesses(buffer.as_mut_ptr(), BUF_SIZE as _, &mut size) == 0 {
            return Err(anyhow!("无法获取所有进程信息"));
        }
    }

    for proc_id in buffer[..size as usize].iter().copied() {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, proc_id);
            if handle.is_null() {
                continue;
            }

            const STR_BUF_SIZE: usize = 1024;
            let mut file_name = [0u16; STR_BUF_SIZE];
            let size = GetModuleFileNameExW(
                handle,
                null_mut(),
                file_name.as_mut_ptr().cast(),
                STR_BUF_SIZE as _,
            );
            CloseHandle(handle);

            if size == STR_BUF_SIZE as _ {
                continue;
            }

            let file_name = String::from_utf16(&file_name[..size as usize])?;

            if file_regex.is_match(&file_name) {
                return Ok(Some(proc_id));
            }
        }
    }

    Ok(None)
}

fn check_alive(proc_id: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION, FALSE, proc_id);

        if handle.is_null() {
            return false;
        }

        let mut exit_code = 0;
        if GetExitCodeProcess(handle, &mut exit_code) == FALSE {
            CloseHandle(handle);
            return false;
        }
        CloseHandle(handle);

        exit_code == STILL_ACTIVE
    }
}

fn recover_on_close() -> Result<()> {
    unsafe extern "system" fn callback(_ctrl_type: DWORD) -> BOOL {
        if let Err(err) = recover_resolution() {
            eprintln!("无法恢复分辨率：{err}");
        }

        std::process::exit(0);
    }

    if unsafe { SetConsoleCtrlHandler(Some(callback), 1) } == FALSE {
        Err(anyhow!("设置关闭回调失败"))
    } else {
        Ok(())
    }
}
