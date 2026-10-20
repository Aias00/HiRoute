use super::*;

pub(super) fn write(stage: &Path, locations: &Value) -> Result<(), LocalStorageError> {
    let locations: hiroute_host_runtime::WindowsDesktopRestore =
        serde_json::from_value(locations.clone()).map_err(|_| LocalStorageError::InvalidData)?;
    locations.validate()?;
    let program = locations.program.display();
    let cli = locations
        .files
        .values()
        .find(|path| path.file_name().is_some_and(|name| name == "hiroute.exe"))
        .ok_or(LocalStorageError::InvalidData)?
        .display();
    let task = &locations.task_name;
    let mut text = format!(
        r"# Windows program and Task recovery / Windows 程序与任务恢复

Read RECOVERY.md or 恢复说明.md first. Use this exact batch and preserve the newer complete data with its matching installer/program separately. Work in the same ordinary Windows account; administrator access is not required.
先阅读 RECOVERY.md 或恢复说明.md。使用本批次，将新版完整数据与配套安装器/程序另存；在同一个普通 Windows 账户中操作，无需管理员权限。

1. Using the current installed CLI, run `hiroute service stop --output json`, then `hiroute service uninstall --output json` to remove its stopped Task registration. Fully quit Desktop and confirm its daemon and CPA have exited. Preserve the current program directories and HKCU HiRoute uninstall entry separately. Do not run the product uninstaller or delete user data.
先用当前 CLI 停止服务并移除已停止的 Task 注册，完全退出桌面端并确认守护进程和 CPA 已结束；另存当前程序目录与当前账户的 HiRoute 卸载注册项，不要运行产品卸载程序或删除用户数据。

If `hiroute` is not on PATH, invoke the installed entry at `{cli}` directly (PowerShell uses `&` before a quoted executable path). If the CLI is damaged, open Task Scheduler under this same account. Verify that the recorded `{task}` Task belongs to this account/current HiRoute installation, export it with the newer program, then stop/disable it and delete only that stopped registration. Confirm its owned processes have exited before copying data. If ownership or process exit cannot be verified, preserve everything and seek support.
若找不到 `hiroute` 命令，请直接运行上面的安装入口（PowerShell 在带引号的程序路径前使用 `&`）。若 CLI 已损坏，在同一账户的任务计划程序中核对本说明记录的 Task 属于当前账户和 HiRoute 安装，将其导出并与新版程序另存，再停止、禁用并仅删除这一已停止的注册；确认其进程均已结束后再复制数据。无法核对归属或退出状态时，请保留现场并寻求支持。

2. Restore the complete original storage group using this batch's recovery guide. Then copy the following entire directories/files, preserving native permissions. The retained native file map includes both CLI/daemon entries, management Skills, package metadata, documentation and licenses.
按本批次说明还原完整旧数据组，再保留原生权限复制下列完整目录和文件。原生文件清单包含 CLI/守护进程入口、管理 Skill、包元数据、文档和许可证。

- recovery-package/program/ → {program}

"
    );
    if let Some(desktop) = &locations.desktop {
        if !desktop.is_absolute() {
            return Err(LocalStorageError::InvalidData);
        }
        text.push_str(&format!(
            "- recovery-package/desktop/ → {}\n",
            desktop.display()
        ));
        if !stage
            .join("recovery-package/installation/uninstall.reg")
            .is_file()
        {
            return Err(LocalStorageError::InvalidData);
        }
        text.push_str(r"
The complete Desktop includes its matching shell, uninstaller and closed inventory. Export the current `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\HiRoute` entry separately. After restoring the old programs, import `recovery-package/installation/uninstall.reg` with Windows `reg.exe import`. This restores the old current-user Apps registration. Do not substitute another account or another revision's installer.
旧桌面包包含配套外壳、卸载程序和完整清单。另行导出当前账户上述 HiRoute 注册项；还原旧程序后，用 Windows `reg.exe import` 导入保留的 uninstall.reg，恢复对应旧版的应用注册。不要换账户或使用其它修订的安装器。

");
    }
    for (name, target) in &locations.files {
        text.push_str(&format!(
            "- recovery-package/{name} → {}\n",
            target.display()
        ));
    }
    let autostart = locations.task_autostart;
    let shell_login = locations.shell_login;
    text.push_str(&format!(r"
3. With the restored old CLI run `hiroute service install --manager task --output json`, then `hiroute service start --output json`. Open the restored Desktop only after the old data and programs are paired. Its original Desktop login preference was {}. Restore that choice using the matching Desktop's login setting, then run `hiroute service autostart {} --output json` last to retain the Task's independently recorded login choice even if it differed from the shell.
使用已还原的旧 CLI 重建 Task 并启动服务，确认完整旧组与旧程序配套后再打开旧桌面端；先在旧桌面端恢复其原先的登录启动选择，最后再运行上述 Task autostart 命令，保留 Task 独立记录的选择（即使它与桌面端原先的选择不同）。

4. Verify `hiroute system status --output json` reports actual Control/Gateway readiness. Confirm the original source, credentials, routing and history, and complete a real request. Task command success alone is insufficient. Post-backup data remains in the separately preserved newer generation.
检查 Control/Gateway 实际就绪、原模型源与凭据、路由和旧历史，并完成一次真实请求；仅 Task 命令成功不代表恢复完成。备份之后的数据保留在另存的新版组中。
", if shell_login { "enabled / 开启" } else { "disabled / 关闭" }, if autostart { "enable" } else { "disable" }));
    write_private(&stage.join("PROGRAM-LOCATIONS.md"), text.as_bytes())
}
